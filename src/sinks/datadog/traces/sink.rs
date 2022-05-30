use std::{fmt::Debug, sync::Arc};

use async_trait::async_trait;
use futures_util::{
    stream::{self, BoxStream},
    StreamExt,
};
use tower::Service;
use vector_core::{
    buffers::Acker,
    config::log_schema,
    event::Event,
    partition::Partitioner,
    sink::StreamSink,
    stream::{BatcherSettings, DriverResponse},
};

use super::service::TraceApiRequest;
use crate::{
    config::SinkContext,
    internal_events::DatadogTracesEncodingError,
    sinks::{datadog::traces::request_builder::DatadogTracesRequestBuilder, util::SinkBuilderExt},
};
#[derive(Default)]
struct EventPartitioner;

// Use all fields from the top level protobuf contruct associated with the API key
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub(crate) struct PartitionKey {
    pub(crate) api_key: Option<Arc<str>>,
    pub(crate) env: Option<String>,
    pub(crate) hostname: Option<String>,
    pub(crate) agent_version: Option<String>,
    pub(crate) target_tps: Option<i64>,
    pub(crate) error_tps: Option<i64>,
}

impl Partitioner for EventPartitioner {
    type Item = Event;
    type Key = PartitionKey;

    fn partition(&self, item: &Self::Item) -> Self::Key {
        match item {
            Event::Metric(_) => {
                panic!("unexpected metric");
            }
            Event::Log(_) => {
                panic!("unexpected log");
            }
            Event::Trace(t) => {
                return PartitionKey {
                    api_key: item.metadata().datadog_api_key().clone(),
                    env: t.get("env").map(|s| s.to_string_lossy()),
                    hostname: t.get(log_schema().host_key()).map(|s| s.to_string_lossy()),
                    agent_version: t.get("agent_version").map(|s| s.to_string_lossy()),
                    target_tps: t.get("target_tps").and_then(|tps| tps.as_integer()),
                    error_tps: t.get("error_tps").and_then(|tps| tps.as_integer()),
                }
            }
        };
    }
}

pub struct TracesSink<S> {
    service: S,
    acker: Acker,
    request_builder: DatadogTracesRequestBuilder,
    batch_settings: BatcherSettings,
}

impl<S> TracesSink<S>
where
    S: Service<TraceApiRequest> + Send,
    S::Error: Debug + Send + 'static,
    S::Future: Send + 'static,
    S::Response: DriverResponse,
{
    pub fn new(
        cx: SinkContext,
        service: S,
        request_builder: DatadogTracesRequestBuilder,
        batch_settings: BatcherSettings,
    ) -> Self {
        TracesSink {
            service,
            acker: cx.acker(),
            request_builder,
            batch_settings,
        }
    }

    async fn run_inner(self: Box<Self>, input: BoxStream<'_, Event>) -> Result<(), ()> {
        let sink = input
            .batched_partitioned(EventPartitioner, self.batch_settings)
            .incremental_request_builder(self.request_builder)
            .flat_map(stream::iter)
            .filter_map(|request| async move {
                match request {
                    Err(e) => {
                        let (message, reason, dropped_events) = e.into_parts();
                        emit!(DatadogTracesEncodingError {
                            message,
                            dropped_events,
                            reason,
                        });
                        None
                    }
                    Ok(req) => Some(req),
                }
            })
            .into_driver(self.service, self.acker);

        sink.run().await
    }
}

#[async_trait]
impl<S> StreamSink<Event> for TracesSink<S>
where
    S: Service<TraceApiRequest> + Send,
    S::Error: Debug + Send + 'static,
    S::Future: Send + 'static,
    S::Response: DriverResponse,
{
    async fn run(self: Box<Self>, input: BoxStream<'_, Event>) -> Result<(), ()> {
        self.run_inner(input).await
    }
}
