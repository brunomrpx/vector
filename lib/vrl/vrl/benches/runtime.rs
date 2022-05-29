use std::collections::{BTreeMap, HashMap};

use compiler::state;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use indoc::indoc;
use vector_common::TimeZone;
use vrl::{Runtime, Value};

struct Source {
    name: &'static str,
    code: &'static str,
}

static SOURCES: [Source; 26] = [
    Source {
        name: "variable",
        code: indoc! {r#"
            foo = {}
        "#},
    },
    Source {
        name: "object",
        code: indoc! {r#"
            {
                "a": "A",
                "c": "C",
                "b": "B",
            }
        "#},
    },
    Source {
        name: "pipelines_lookup",
        code: indoc! {r#"
            lookup = {
                "api_limit" : " The maximum number of requests to the Authentication API in given time has reached.",
                "cls" : " Passwordless login code/link has been sent",
                "coff" : " AD/LDAP Connector is offline ",
                "con" : " AD/LDAP Connector is online and working",
                "cs" : " Passwordless login code has been sent",
                "du" : " User has been deleted.",
                "fce" : " Failed to change user email",
                "fco" : " Origin is not in the Allowed Origins list for the specified application",
                "fcpro" : " Failed to provision a AD/LDAP connector",
                "fcu" : " Failed to change username",
                "fd" : " Failed to generate delegation token",
                "fdeac" : " Failed to activate device.",
                "fdeaz" : " Device authorization request failed.",
                "fdecc" : " User did not confirm device.",
                "feacft" : " Failed to exchange authorization code for Access Token",
                "feccft" : " Failed exchange of Access Token for a Client Credentials Grant",
                "fede" : " Failed to exchange Device Code for Access Token",
                "fens" : " Failed exchange for Native Social Login",
                "feoobft" : " Failed exchange of Password and OOB Challenge for Access Token",
                "feotpft" : " Failed exchange of Password and OTP Challenge for Access Token",
                "fepft" : "Failed exchange of Password for Access Token",
                "fercft" : " Failed Exchange of Password and MFA Recovery code for Access Token",
                "fertft" : " Failed Exchange of Refresh Token for Access Token",
                "flo" : " User logout failed",
                "fn" : " Failed to send email notification",
                "fui" : " Failed to import users",
                "fv" : " Failed to send verification email",
                "fvr" : " Failed to process verification email request",
                "gd_auth_failed" : " One-time password authentication failed.",
                "gd_auth_rejected" : " One-time password authentication rejected.",
                "gd_auth_succeed" : " One-time password authentication success.",
                "gd_recovery_failed" : " Multi-factor recovery code failed.",
                "gd_recovery_rate_limit_exceed" : " Multi-factor recovery code has failed too many times.",
                "gd_recovery_succeed" : " Multi-factor recovery code succeeded authorization.",
                "gd_send_pn" : " Push notification for MFA sent successfully sent.",
                "gd_send_sms" : " SMS for MFA sent successfully sent.",
                "gd_start_auth" : " Second factor authentication event started for MFA.",
                "gd_start_enroll" : " Multi-factor authentication enroll has started.",
                "gd_unenroll" : " Device used for second factor authentication has been unenrolled.",
                "gd_update_device_account" : " Device used for second factor authentication has been updated.",
                "gd_user_delete" : " Deleted multi-factor user account.",
                "limit_delegation" : " Rate limit exceeded to /delegation endpoint",
                "limit_mu" : " An IP address is blocked with 100 failed login attempts using different usernames all with incorrect passwords in 24 hours or 50 sign-up attempts per minute from the same IP address.",
                "limit_wc" : " An IP address is blocked with 10 failed login attempts into a single account from the same IP address.",
                "pwd_leak" : " Someone behind the IP address: ip attempted to login with a leaked password.",
                "s" : " Successful login event.",
                "sdu" : " User successfully deleted",
                "seacft" : " Successful exchange of authorization code for Access Token",
                "seccft" : " Successful exchange of Access Token for a Client Credentials Grant",
                "sede" : " Successful exchange of device code for Access Token",
                "sens" : " Native Social Login",
                "seoobft" : " Successful exchange of Password and OOB Challenge for Access Token",
                "seotpft" : " Successful exchange of Password and OTP Challenge for Access Token",
                "sepft" : " Successful exchange of Password for Access Token",
                "sercft" : " Successful exchange of Password and MFA Recovery code for Access Token",
                "sertft" : " Successful exchange of Refresh Token for Access Token",
                "slo" : " User successfully logged out",
                "sui" : " Successfully imported users",
                "ublkdu" : " User block setup by anomaly detection has been released"
            }
            if (lookup_value, err = get(lookup, [.custom.data.type]); lookup_value != null) {
                .custom.message = lookup_value
            }
        "#},
    },
    Source {
        name: "parse_json",
        code: indoc! {r#"
            .result, .err = parse_json("{")
            [.result, .err]
        "#},
    },
    Source {
        name: "parse_groks_bla",
        code: indoc! {r#"
            .custom.message = "INFO  [MemtableFlushWriter:1] 2016-06-28 16:19:48,627  Memtable.java:382 - Completed flushing /app/cassandra/datastax/dse-data01/system/local-7ad54392bcdd35a684174e047860b377/system-local-tmp-ka-3981-Data.db (0.000KiB) for commitlog position ReplayPosition(segmentId=1467130786324, position=567)"
            parse_groks!(value: .custom.message,
                patterns: [
                    "(?s)%{_prefix} %{regex(\"Compacting\"):db.operation}.* %{_keyspace}/%{_table}:%{data:partition_key} \\(%{_bytes} bytes\\)",
                    "(?s)%{_prefix} %{regex(\"Flushing\"):db.operation}.*\\(Keyspace='%{_keyspace}', ColumnFamily='%{_table}'\\) %{data}: %{_onheap_total}/%{_offheap_total}, live: %{_onheap_live}/%{_offheap_live}, flushing: %{_onheap_flush}/%{_offheap_flush}, this: %{_onheap_this}/%{_offheap_this}",
                    "(?s)%{_prefix} %{regex(\"Enqueuing\"):db.operation}.* of %{_keyspace}: %{_onheap_bytes}%{data} \\(%{_onheap_pct}%\\) on-heap, %{_offheap_bytes} \\(%{_offheap_pct}%\\).*",
                    "(?s)%{_prefix} %{regex(\"Writing\"):db.operation}.*-%{_keyspace}%{data}\\(%{number:cassandra.bytes:scale(1000000)}%{data}, %{integer:cassandra.ops} ops, %{_onheap_pct}%/%{_offheap_pct}.*",
                    "(?s)%{_prefix} Completed %{regex(\"flushing\"):db.operation} %{_sstable} \\(%{number:cassandra.bytes_kb}KiB\\) for commitlog %{data:commitlog}",
                    "(?s)%{_prefix}\\s+%{regex(\"Compacted\"):db.operation}.* to \\[%{_sstable}\\].\\s+%{notSpace:cassandra.bytes_in} bytes to %{notSpace:cassandra.bytes_out} \\(\\~%{integer:cassandra.percent_of_orig}% of original\\) in %{notSpace:cassandra.duration_ms}ms = %{number:cassandra.speed_mb}MB/s.\\s+%{notSpace:cassandra.pkeys_in} total partitions merged to %{notSpace:cassandra.pkeys_out}\\.\\s+Partition merge counts were %{data:cassandra.merge_cnt}",
                    "(?s)%{_prefix} G.* %{integer:duration:scale(1000000)}ms. %{data}: %{integer:cassandra.eden.orig_bytes} -> %{integer:cassandra.eden.new_bytes}; %{data}: %{integer:cassandra.oldgen.orig_bytes} -> %{integer:cassandra.oldgen.new_bytes};.*",
                    "(?s)%{_prefix} %{word:cassandra.pool}\\s*(?>%{integer:cassandra.cache_used}\\s*%{integer:cassandra.cache_size}\\s*all|%{integer:cassandra.threads.active}\\s*%{integer:cassandra.threads.pending}\\s*%{integer:cassandra.threads.completed}\\s*%{integer:cassandra.threads.blocked}\\s*%{integer:cassandra.threads.all_time_blocked}|%{integer:cassandra.threads.active}\\s*%{integer:cassanadra.threads.pending})",
                    "(?s)%{_prefix} %{integer:db.operations} operations were slow in the last %{integer:elapsed_time:scale(1000000)} msecs:\\n%{data:db.slow_statements:array(\"\", \"\\\\n\")}",
                    "(?s)%{_prefix} %{data:msg}",
                ],
                aliases: {
                    "cassandra_compaction_key": "%{_prefix} %{regex(\"Compacting\"):db.operation}.* %{_keyspace}/%{_table}:%{data:partition_key} \\(%{_bytes} bytes\\)",
                    "cassandra_pool_cleaner": "%{_prefix} %{regex(\"Flushing\"):db.operation}.*\\(Keyspace='%{_keyspace}', ColumnFamily='%{_table}'\\) %{data}: %{_onheap_total}/%{_offheap_total}, live: %{_onheap_live}/%{_offheap_live}, flushing: %{_onheap_flush}/%{_offheap_flush}, this: %{_onheap_this}/%{_offheap_this}",
                    "cassandra_pool_cleaner2": "%{_prefix} %{regex(\"Enqueuing\"):db.operation}.* of %{_keyspace}: %{_onheap_bytes}%{data} \\(%{_onheap_pct}%\\) on-heap, %{_offheap_bytes} \\(%{_offheap_pct}%\\).*",
                    "cassandra_table_flush": "%{_prefix} %{regex(\"Writing\"):db.operation}.*-%{_keyspace}%{data}\\(%{number:cassandra.bytes:scale(1000000)}%{data}, %{integer:cassandra.ops} ops, %{_onheap_pct}%/%{_offheap_pct}.*",
                    "cassandra_mem_flush": "%{_prefix} Completed %{regex(\"flushing\"):db.operation} %{_sstable} \\(%{number:cassandra.bytes_kb}KiB\\) for commitlog %{data:commitlog}",
                    "cassandra_compaction": "%{_prefix}\\s+%{regex(\"Compacted\"):db.operation}.* to \\[%{_sstable}\\].\\s+%{notSpace:cassandra.bytes_in} bytes to %{notSpace:cassandra.bytes_out} \\(\\~%{integer:cassandra.percent_of_orig}% of original\\) in %{notSpace:cassandra.duration_ms}ms = %{number:cassandra.speed_mb}MB/s.\\s+%{notSpace:cassandra.pkeys_in} total partitions merged to %{notSpace:cassandra.pkeys_out}\\.\\s+Partition merge counts were %{data:cassandra.merge_cnt}",
                    "cassandra_gc_format": "%{_prefix} G.* %{integer:duration:scale(1000000)}ms. %{data}: %{integer:cassandra.eden.orig_bytes} -> %{integer:cassandra.eden.new_bytes}; %{data}: %{integer:cassandra.oldgen.orig_bytes} -> %{integer:cassandra.oldgen.new_bytes};.*",
                    "cassandra_thread_pending": "%{_prefix} %{word:cassandra.pool}\\s*(?>%{integer:cassandra.cache_used}\\s*%{integer:cassandra.cache_size}\\s*all|%{integer:cassandra.threads.active}\\s*%{integer:cassandra.threads.pending}\\s*%{integer:cassandra.threads.completed}\\s*%{integer:cassandra.threads.blocked}\\s*%{integer:cassandra.threads.all_time_blocked}|%{integer:cassandra.threads.active}\\s*%{integer:cassanadra.threads.pending})",
                    "cassandra_slow_statements": "%{_prefix} %{integer:db.operations} operations were slow in the last %{integer:elapsed_time:scale(1000000)} msecs:\\n%{data:db.slow_statements:array(\"\", \"\\\\n\")}",
                    "cassandra_fallback_parser": "%{_prefix} %{data:msg}",
                    "_level": "%{word:db.severity}",
                    "_thread_name": "%{notSpace:logger.thread_name}",
                    "_thread_id": "%{integer:logger.thread_id}",
                    "_logger_name": "%{notSpace:logger.name}",
                    "_table": "%{word:db.table}",
                    "_sstable": "%{notSpace:cassandra.sstable}",
                    "_bytes": "%{integer:cassandra.bytes}",
                    "_keyspace": "%{word:cassandra.keyspace}",
                    "_onheap_total": "%{number:cassandra.onheap.total}",
                    "_onheap_live": "%{number:cassandra.onheap.live}",
                    "_onheap_flush": "%{number:cassandra.onheap.flush}",
                    "_onheap_this": "%{number:cassandra.onheap.this}",
                    "_onheap_bytes": "%{integer:cassandra.onheap.bytes}",
                    "_onheap_pct": "%{integer:cassandra.onheap.percent}",
                    "_offheap_total": "%{number:cassandra.offheap.total}",
                    "_offheap_live": "%{number:cassandra.offheap.live}",
                    "_offheap_flush": "%{number:cassandra.offheap.flush}",
                    "_offheap_this": "%{number:cassandra.offheap.this}",
                    "_offheap_bytes": "%{integer:cassandra.offheap.bytes}",
                    "_offheap_pct": "%{integer:cassandra.offheap.percent}",
                    "_default_prefix": "%{_level}\\s+\\[(%{_thread_name}:%{_thread_id}|%{_thread_name})\\]\\s+%{date(\"yyyy-MM-dd HH:mm:ss,SSS\"):db.date}\\s+%{word:filename}.java:%{integer:lineno} -",
                    "_suggested_prefix": "%{date(\"yyyy-MM-dd HH:mm:ss\"):db.date} \\[(%{_thread_name}:%{_thread_id}|%{_thread_name})\\] %{_level} %{_logger_name}\\s+-",
                    "_prefix": "(?>%{_default_prefix}|%{_suggested_prefix})"
                }
            )
        "#},
    },
    Source {
        name: "if_false",
        code: indoc! {r#"
            if (.foo != null) {
                .derp = 123
            }
        "#},
    },
    Source {
        name: "merge",
        code: indoc! {r#"
            merge({ "a": 1, "b": 2 }, { "b": 3, "c": 4 })
        "#},
    },
    Source {
        name: "parse_groks",
        code: indoc! {r#"
            parse_groks!(
                "2020-10-02T23:22:12.223222Z info hello world",
                patterns: [
                    "%{common_prefix} %{_status} %{_message}",
                    "%{common_prefix} %{_message}"
                ],
                aliases: {
                    "common_prefix": "%{_timestamp} %{_loglevel}",
                    "_timestamp": "%{TIMESTAMP_ISO8601:timestamp}",
                    "_loglevel": "%{LOGLEVEL:level}",
                    "_status": "%{POSINT:status}",
                    "_message": "%{GREEDYDATA:message}"
                }
            )
        "#},
    },
    Source {
        name: "pipelines_grok",
        code: indoc! {r#"
            custom, err = parse_groks(value: .custom.message,
                patterns: [
                    "(?s)%{_prefix} %{regex(\"Compacting\"):db.operation}.* %{_keyspace}/%{_table}:%{data:partition_key} \\(%{_bytes} bytes\\)",
                    "(?s)%{_prefix} %{regex(\"Flushing\"):db.operation}.*\\(Keyspace='%{_keyspace}', ColumnFamily='%{_table}'\\) %{data}: %{_onheap_total}/%{_offheap_total}, live: %{_onheap_live}/%{_offheap_live}, flushing: %{_onheap_flush}/%{_offheap_flush}, this: %{_onheap_this}/%{_offheap_this}",
                    "(?s)%{_prefix} %{regex(\"Enqueuing\"):db.operation}.* of %{_keyspace}: %{_onheap_bytes}%{data} \\(%{_onheap_pct}%\\) on-heap, %{_offheap_bytes} \\(%{_offheap_pct}%\\).*",
                    "(?s)%{_prefix} %{regex(\"Writing\"):db.operation}.*-%{_keyspace}%{data}\\(%{number:cassandra.bytes:scale(1000000)}%{data}, %{integer:cassandra.ops} ops, %{_onheap_pct}%/%{_offheap_pct}.*",
                    "(?s)%{_prefix} Completed %{regex(\"flushing\"):db.operation} %{_sstable} \\(%{number:cassandra.bytes_kb}KiB\\) for commitlog %{data:commitlog}",
                    "(?s)%{_prefix}\\s+%{regex(\"Compacted\"):db.operation}.* to \\[%{_sstable}\\].\\s+%{notSpace:cassandra.bytes_in} bytes to %{notSpace:cassandra.bytes_out} \\(\\~%{integer:cassandra.percent_of_orig}% of original\\) in %{notSpace:cassandra.duration_ms}ms = %{number:cassandra.speed_mb}MB/s.\\s+%{notSpace:cassandra.pkeys_in} total partitions merged to %{notSpace:cassandra.pkeys_out}\\.\\s+Partition merge counts were %{data:cassandra.merge_cnt}",
                    "(?s)%{_prefix} G.* %{integer:duration:scale(1000000)}ms. %{data}: %{integer:cassandra.eden.orig_bytes} -> %{integer:cassandra.eden.new_bytes}; %{data}: %{integer:cassandra.oldgen.orig_bytes} -> %{integer:cassandra.oldgen.new_bytes};.*",
                    "(?s)%{_prefix} %{word:cassandra.pool}\\s*(?>%{integer:cassandra.cache_used}\\s*%{integer:cassandra.cache_size}\\s*all|%{integer:cassandra.threads.active}\\s*%{integer:cassandra.threads.pending}\\s*%{integer:cassandra.threads.completed}\\s*%{integer:cassandra.threads.blocked}\\s*%{integer:cassandra.threads.all_time_blocked}|%{integer:cassandra.threads.active}\\s*%{integer:cassanadra.threads.pending})",
                    "(?s)%{_prefix} %{integer:db.operations} operations were slow in the last %{integer:elapsed_time:scale(1000000)} msecs:\\n%{data:db.slow_statements:array(\"\", \"\\\\n\")}",
                    "(?s)%{_prefix} %{data:msg}",
                ],
                aliases: {
                    "cassandra_compaction_key": "%{_prefix} %{regex(\"Compacting\"):db.operation}.* %{_keyspace}/%{_table}:%{data:partition_key} \\(%{_bytes} bytes\\)",
                    "cassandra_pool_cleaner": "%{_prefix} %{regex(\"Flushing\"):db.operation}.*\\(Keyspace='%{_keyspace}', ColumnFamily='%{_table}'\\) %{data}: %{_onheap_total}/%{_offheap_total}, live: %{_onheap_live}/%{_offheap_live}, flushing: %{_onheap_flush}/%{_offheap_flush}, this: %{_onheap_this}/%{_offheap_this}",
                    "cassandra_pool_cleaner2": "%{_prefix} %{regex(\"Enqueuing\"):db.operation}.* of %{_keyspace}: %{_onheap_bytes}%{data} \\(%{_onheap_pct}%\\) on-heap, %{_offheap_bytes} \\(%{_offheap_pct}%\\).*",
                    "cassandra_table_flush": "%{_prefix} %{regex(\"Writing\"):db.operation}.*-%{_keyspace}%{data}\\(%{number:cassandra.bytes:scale(1000000)}%{data}, %{integer:cassandra.ops} ops, %{_onheap_pct}%/%{_offheap_pct}.*",
                    "cassandra_mem_flush": "%{_prefix} Completed %{regex(\"flushing\"):db.operation} %{_sstable} \\(%{number:cassandra.bytes_kb}KiB\\) for commitlog %{data:commitlog}",
                    "cassandra_compaction": "%{_prefix}\\s+%{regex(\"Compacted\"):db.operation}.* to \\[%{_sstable}\\].\\s+%{notSpace:cassandra.bytes_in} bytes to %{notSpace:cassandra.bytes_out} \\(\\~%{integer:cassandra.percent_of_orig}% of original\\) in %{notSpace:cassandra.duration_ms}ms = %{number:cassandra.speed_mb}MB/s.\\s+%{notSpace:cassandra.pkeys_in} total partitions merged to %{notSpace:cassandra.pkeys_out}\\.\\s+Partition merge counts were %{data:cassandra.merge_cnt}",
                    "cassandra_gc_format": "%{_prefix} G.* %{integer:duration:scale(1000000)}ms. %{data}: %{integer:cassandra.eden.orig_bytes} -> %{integer:cassandra.eden.new_bytes}; %{data}: %{integer:cassandra.oldgen.orig_bytes} -> %{integer:cassandra.oldgen.new_bytes};.*",
                    "cassandra_thread_pending": "%{_prefix} %{word:cassandra.pool}\\s*(?>%{integer:cassandra.cache_used}\\s*%{integer:cassandra.cache_size}\\s*all|%{integer:cassandra.threads.active}\\s*%{integer:cassandra.threads.pending}\\s*%{integer:cassandra.threads.completed}\\s*%{integer:cassandra.threads.blocked}\\s*%{integer:cassandra.threads.all_time_blocked}|%{integer:cassandra.threads.active}\\s*%{integer:cassanadra.threads.pending})",
                    "cassandra_slow_statements": "%{_prefix} %{integer:db.operations} operations were slow in the last %{integer:elapsed_time:scale(1000000)} msecs:\\n%{data:db.slow_statements:array(\"\", \"\\\\n\")}",
                    "cassandra_fallback_parser": "%{_prefix} %{data:msg}",
                    "_level": "%{word:db.severity}",
                    "_thread_name": "%{notSpace:logger.thread_name}",
                    "_thread_id": "%{integer:logger.thread_id}",
                    "_logger_name": "%{notSpace:logger.name}",
                    "_table": "%{word:db.table}",
                    "_sstable": "%{notSpace:cassandra.sstable}",
                    "_bytes": "%{integer:cassandra.bytes}",
                    "_keyspace": "%{word:cassandra.keyspace}",
                    "_onheap_total": "%{number:cassandra.onheap.total}",
                    "_onheap_live": "%{number:cassandra.onheap.live}",
                    "_onheap_flush": "%{number:cassandra.onheap.flush}",
                    "_onheap_this": "%{number:cassandra.onheap.this}",
                    "_onheap_bytes": "%{integer:cassandra.onheap.bytes}",
                    "_onheap_pct": "%{integer:cassandra.onheap.percent}",
                    "_offheap_total": "%{number:cassandra.offheap.total}",
                    "_offheap_live": "%{number:cassandra.offheap.live}",
                    "_offheap_flush": "%{number:cassandra.offheap.flush}",
                    "_offheap_this": "%{number:cassandra.offheap.this}",
                    "_offheap_bytes": "%{integer:cassandra.offheap.bytes}",
                    "_offheap_pct": "%{integer:cassandra.offheap.percent}",
                    "_default_prefix": "%{_level}\\s+\\[(%{_thread_name}:%{_thread_id}|%{_thread_name})\\]\\s+%{date(\"yyyy-MM-dd HH:mm:ss,SSS\"):db.date}\\s+%{word:filename}.java:%{integer:lineno} -",
                    "_suggested_prefix": "%{date(\"yyyy-MM-dd HH:mm:ss\"):db.date} \\[(%{_thread_name}:%{_thread_id}|%{_thread_name})\\] %{_level} %{_logger_name}\\s+-",
                    "_prefix": "(?>%{_default_prefix}|%{_suggested_prefix})"
                }
            )
            if (err == null) {
                .custom, err = merge(.custom, custom, deep: true)
            }
        "#},
    },
    Source {
        name: "pipelines",
        code: indoc! {r#"
            status = string(.custom.http.status_category) ?? string(.custom.level) ?? ""
            status = downcase(status)
            if status == "" {
                .status = 6
            } else {
                if starts_with(status, "f") || starts_with(status, "emerg") {
                    .status = 0
                } else if starts_with(status, "a") {
                    .status = 1
                } else if starts_with(status, "c") {
                    .status = 2
                } else if starts_with(status, "e") {
                    .status = 3
                } else if starts_with(status, "w") {
                    .status = 4
                } else if starts_with(status, "n") {
                    .status = 5
                } else if starts_with(status, "i") {
                    .status = 6
                } else if starts_with(status, "d") || starts_with(status, "trace") || starts_with(status, "verbose") {
                    .status = 7
                } else if starts_with(status, "o") || starts_with(status, "s") || status == "ok" || status == "success" {
                    .status = 8
                }
            }
        "#},
    },
    Source {
        name: "add_bytes",
        code: indoc! {r#"
            . = "hello" + "world"
        "#},
    },
    Source {
        name: "add",
        code: indoc! {r#"
            . = 1 + 2
        "#},
    },
    Source {
        name: "derp",
        code: indoc! {r#"
            .foo = { "foo": 123 }
            .matches = { "num": "2", "name": .message }
        "#},
    },
    Source {
        name: "simple",
        code: indoc! {r#"
            .hostname = "vector"

            if .status == "warning" {
                .thing = upcase(.hostname)
            } else if .status == "notice" {
                .thung = downcase(.hostname)
            } else {
                .nong = upcase(.hostname)
            }

            .matches = { "name": .message, "num": "2" }
            .origin, .err = .hostname + "/" + .matches.name + "/" + .matches.num
        "#},
    },
    Source {
        name: "starts_with",
        code: indoc! {r#"
            status = string(.foo) ?? ""
            .status = starts_with("a", status)
        "#},
    },
    Source {
        name: "11",
        code: indoc! {r#"
            .hostname = "vector"

            if .status == "warning" {
                .thing = upcase(.hostname)
            } else if .status == "notice" {
                .thung = downcase(.hostname)
            } else {
                .nong = upcase(.hostname)
            }
        "#},
    },
    Source {
        name: "10",
        code: indoc! {r#"
            .foo = {
                "a": 123,
                "b": 456,
            }
        "#},
    },
    Source {
        name: "9",
        code: indoc! {r#"
            upcase("hi")
        "#},
    },
    Source {
        name: "8",
        code: indoc! {r#"
            123
        "#},
    },
    Source {
        name: "7",
        code: indoc! {r#"
            .foo == "hi"
        "#},
    },
    Source {
        name: "6",
        code: indoc! {r#"
            derp = "hi!"
        "#},
    },
    Source {
        name: "5",
        code: indoc! {r#"
            .derp = "hi!"
        "#},
    },
    Source {
        name: "4",
        code: indoc! {r#"
            .derp
        "#},
    },
    Source {
        name: "3",
        code: indoc! {r#"
            .
        "#},
    },
    Source {
        name: "parse_json",
        code: indoc! {r#"
            x = parse_json!(s'{"noog": "nork"}')
            x.noog
        "#},
    },
    Source {
        name: "0",
        code: indoc! {r#"
            uuid_v4()
        "#},
    },
];

fn benchmark_kind_display(c: &mut Criterion) {
    /*
    {
        use inkwell::context::Context;
        use inkwell::targets::{InitializationConfig, Target};
        use inkwell::OptimizationLevel;
        Target::initialize_native(&InitializationConfig::default()).unwrap();
        let context = Context::create();
        let module = context.create_module("test");
        let builder = context.create_builder();

        // Set up the function signature
        let double = context.f64_type();
        let sig = double.fn_type(&[], false);

        // Add the function to our module
        let f = module.add_function("test_fn", sig, None);
        let b = context.append_basic_block(f, "entry");
        builder.position_at_end(b);

        let function_name = "derp".to_owned();
        let function_type = context.void_type().fn_type(&[], false);
        let fn_impl = module.add_function(&function_name, function_type, None);
        builder.build_call(fn_impl, &[], &function_name);

        {
            let function_name = "vrl_fn_uuid_v4".to_owned();
            let function_type = context.void_type().fn_type(&[], false);
            let fn_impl = module.add_function(&function_name, function_type, None);
            builder.build_call(fn_impl, &[], &function_name);
        }

        // Insert a return statement
        let ret = double.const_float(64.0);
        builder.build_return(Some(&ret));

        println!("{}", module.print_to_string().to_string());

        // create the JIT engine
        let mut ee = module
            .create_jit_execution_engine(OptimizationLevel::None)
            .unwrap();

        // fetch our JIT'd function and execute it
        unsafe {
            let test_fn = ee
                .get_function::<unsafe extern "C" fn() -> f64>("test_fn")
                .unwrap();
            let return_value = test_fn.call();
            assert_eq!(return_value, 64.0);
        }
    }
    */

    let mut group = c.benchmark_group("vrl/runtime");
    for source in &SOURCES {
        let state = state::Runtime::default();
        let runtime = Runtime::new(state);
        let tz = TimeZone::default();
        let functions = vrl_stdlib::all();
        let mut external_env = state::ExternalEnv::default();
        let (program, mut local_env) =
            vrl::compile_with_state(source.code, &functions, &mut external_env).unwrap();
        let vm = runtime
            .compile(functions, &program, &mut external_env)
            .unwrap();
        let builder = vrl::llvm::Compiler::new().unwrap();
        println!("bench 1");
        let mut symbols = HashMap::new();
        symbols.insert("vrl_fn_downcase", vrl_stdlib::vrl_fn_downcase as usize);
        symbols.insert("vrl_fn_merge", vrl_stdlib::vrl_fn_merge as usize);
        symbols.insert("vrl_fn_get", vrl_stdlib::vrl_fn_get as usize);
        symbols.insert(
            "vrl_fn_parse_groks",
            vrl_stdlib::vrl_fn_parse_groks as usize,
        );
        symbols.insert("vrl_fn_parse_json", vrl_stdlib::vrl_fn_parse_json as usize);
        symbols.insert(
            "vrl_fn_starts_with",
            vrl_stdlib::vrl_fn_starts_with as usize,
        );
        symbols.insert("vrl_fn_string", vrl_stdlib::vrl_fn_string as usize);
        symbols.insert("vrl_fn_upcase", vrl_stdlib::vrl_fn_upcase as usize);
        let library = builder
            .compile(
                (&mut local_env, &mut external_env),
                &program,
                vrl_stdlib::all(),
                symbols,
            )
            .unwrap();
        println!("bench 2");
        let execute = library.get_function().unwrap();
        println!("bench 3");

        {
            println!("yo");
            let mut obj = Value::Object(BTreeMap::default());
            let mut context = core::Context {
                target: &mut obj,
                timezone: &tz,
            };
            let mut result = Ok(Value::Null);
            println!("bla");
            unsafe { execute.call(&mut context, &mut result) };
            println!("derp");
        }

        {
            let mut obj = Value::Object(BTreeMap::default());
            let mut context = core::Context {
                target: &mut obj,
                timezone: &tz,
            };
            let mut result = Ok(Value::Null);
            unsafe { execute.call(&mut context, &mut result) };

            println!("LLVM obj: {}", obj);
            println!("LLVM result: {:?}", result);
        }

        {
            let state = state::Runtime::default();
            let mut runtime = Runtime::new(state);
            let mut obj = Value::Object(BTreeMap::default());
            let result = runtime.run_vm(&vm, &mut obj, &tz);
            runtime.clear();

            println!("VM obj: {}", obj);
            println!("VM result: {:?}", result);
        }

        {
            let state = state::Runtime::default();
            let mut runtime = Runtime::new(state);
            let mut obj = Value::Object(BTreeMap::default());
            let result = runtime.resolve(&mut obj, &program, &tz);
            runtime.clear();

            println!("AST obj: {}", obj);
            println!("AST result: {:?}", result);
        }

        group.bench_with_input(
            BenchmarkId::new("LLVM", source.name),
            &execute,
            |b, execute| {
                b.iter_with_setup(
                    || Value::Object(BTreeMap::default()),
                    |mut obj| {
                        {
                            let mut context = core::Context {
                                target: &mut obj,
                                timezone: &tz,
                            };
                            let mut result = Ok(Value::Null);
                            unsafe { execute.call(&mut context, &mut result) };
                        }
                        obj // Return the obj so it doesn't get dropped.
                    },
                )
            },
        );

        group.bench_with_input(BenchmarkId::new("VM", source.name), &vm, |b, vm| {
            let state = state::Runtime::default();
            let mut runtime = Runtime::new(state);
            b.iter_with_setup(
                || Value::Object(BTreeMap::default()),
                |mut obj| {
                    let _ = black_box(runtime.run_vm(vm, &mut obj, &tz));
                    runtime.clear();
                    obj // Return the obj so it doesn't get dropped.
                },
            )
        });

        group.bench_with_input(BenchmarkId::new("Ast", source.name), &(), |b, _| {
            let state = state::Runtime::default();
            let mut runtime = Runtime::new(state);
            b.iter_with_setup(
                || Value::Object(BTreeMap::default()),
                |mut obj| {
                    let _ = black_box(runtime.resolve(&mut obj, &program, &tz));
                    runtime.clear();
                    obj
                },
            )
        });
    }
}

criterion_group!(name = vrl_compiler_kind;
                 config = Criterion::default();
                 targets = benchmark_kind_display);
criterion_main!(vrl_compiler_kind);
