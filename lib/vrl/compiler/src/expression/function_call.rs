use std::{any::Any, fmt, sync::Arc};

use anymap::AnyMap;
use diagnostic::{DiagnosticError, Label, Note, Urls};

use crate::{
    expression::{levenstein, ExpressionError, FunctionArgument, Noop},
    function::{ArgumentList, FunctionCompileContext, Parameter, ResolvedArgument},
    parser::{Ident, Node},
    state::{ExternalEnv, LocalEnv},
    value::Kind,
    vm::OpCode,
    Context, Expression, Function, Resolved, Span, TypeDef,
};

#[derive(Clone)]
pub struct FunctionCall {
    abort_on_error: bool,
    expr: Box<dyn Expression>,
    maybe_fallible_arguments: bool,

    // used for enhancing runtime error messages (using abort-instruction).
    //
    // TODO: have span store line/col details to further improve this.
    span: Span,

    // used for equality check
    ident: &'static str,

    // The index of the function in the list of stdlib functions.
    // Used by the VM to identify this function when called.
    function_id: usize,
    arguments: Arc<Vec<Node<FunctionArgument>>>,
}

impl FunctionCall {
    pub fn new(
        call_span: Span,
        ident: Node<Ident>,
        abort_on_error: bool,
        arguments: Vec<Node<FunctionArgument>>,
        funcs: &[Box<dyn Function>],
        local: &mut LocalEnv,
        external: &mut ExternalEnv,
    ) -> Result<Self, Error> {
        let (ident_span, ident) = ident.take();

        // Check if function exists.
        let (function_id, function) = match funcs
            .iter()
            .enumerate()
            .find(|(_pos, f)| f.identifier() == ident.as_ref())
        {
            Some(function) => function,
            None => {
                let idents = funcs
                    .iter()
                    .map(|func| func.identifier())
                    .collect::<Vec<_>>();

                return Err(Error::Undefined {
                    ident_span,
                    ident: ident.clone(),
                    idents,
                });
            }
        };

        // Check function arity.
        if arguments.len() > function.parameters().len() {
            let arguments_span = {
                let start = arguments.first().unwrap().span().start();
                let end = arguments.last().unwrap().span().end();

                Span::new(start, end)
            };

            return Err(Error::WrongNumberOfArgs {
                arguments_span,
                max: function.parameters().len(),
            });
        }

        // Keeps track of positional argument indices.
        //
        // Used to map a positional argument to its keyword. Keyword arguments
        // can be used in any order, and don't count towards the index of
        // positional arguments.
        let mut index = 0;
        let mut list = ArgumentList::default();

        let mut maybe_fallible_arguments = false;
        for node in &arguments {
            let (argument_span, argument) = node.clone().take();

            let parameter = match argument.keyword() {
                // positional argument
                None => {
                    index += 1;
                    function.parameters().get(index - 1)
                }

                // keyword argument
                Some(k) => function
                    .parameters()
                    .iter()
                    .enumerate()
                    .find(|(_, param)| param.keyword == k)
                    .map(|(pos, param)| {
                        if pos == index {
                            index += 1;
                        }

                        param
                    }),
            }
            .ok_or_else(|| Error::UnknownKeyword {
                keyword_span: argument.keyword_span().expect("exists"),
                ident_span,
                keywords: function.parameters().iter().map(|p| p.keyword).collect(),
            })?;

            // Check if the argument is of the expected type.
            let argument_type_def = argument.type_def((local, external));
            let expr_kind = argument_type_def.kind();
            let param_kind = parameter.kind();

            if !param_kind.intersects(expr_kind) {
                return Err(Error::InvalidArgumentKind {
                    function_ident: function.identifier(),
                    abort_on_error,
                    arguments_fmt: arguments
                        .iter()
                        .map(|arg| arg.inner().to_string())
                        .collect::<Vec<_>>(),
                    parameter: *parameter,
                    got: expr_kind.clone(),
                    argument,
                    argument_span,
                });
            } else if !param_kind.is_superset(expr_kind) {
                maybe_fallible_arguments = true;
            }

            // Check if the argument is infallible.
            if argument_type_def.is_fallible() {
                return Err(Error::FallibleArgument {
                    expr_span: argument.span(),
                });
            }

            list.insert(parameter.keyword, argument.into_inner());
        }

        // Check missing required arguments.
        function
            .parameters()
            .iter()
            .enumerate()
            .filter(|(_, p)| p.required)
            .filter(|(_, p)| !list.keywords().contains(&p.keyword))
            .try_for_each(|(i, p)| -> Result<_, _> {
                Err(Error::MissingArgument {
                    call_span,
                    keyword: p.keyword,
                    position: i,
                })
            })?;

        // We take the external context, and pass it to the function compile context, this allows
        // functions mutable access to external state, but keeps the internal compiler state behind
        // an immutable reference, to ensure compiler state correctness.
        let external_context = external.swap_external_context(AnyMap::new());

        let mut compile_ctx =
            FunctionCompileContext::new(call_span).with_external_context(external_context);

        let mut expr = function
            .compile((local, external), &mut compile_ctx, list)
            .map_err(|error| Error::Compilation { call_span, error })?;

        // Re-insert the external context into the compiler state.
        let _ = external.swap_external_context(compile_ctx.into_external_context());

        // Asking for an infallible function to abort on error makes no sense.
        // We consider this an error at compile-time, because it makes the
        // resulting program incorrectly convey this function call might fail.
        if abort_on_error
            && !maybe_fallible_arguments
            && !expr.type_def((local, external)).is_fallible()
        {
            return Err(Error::AbortInfallible {
                ident_span,
                abort_span: Span::new(ident_span.end(), ident_span.end() + 1),
            });
        }

        // Update the state if necessary.
        expr.update_state(local, external)
            .map_err(|err| Error::UpdateState {
                call_span,
                error: err.to_string(),
            })?;

        Ok(Self {
            abort_on_error,
            expr,
            maybe_fallible_arguments,
            span: call_span,
            ident: function.identifier(),
            function_id,
            arguments: Arc::new(arguments),
        })
    }

    fn compile_arguments(
        &self,
        function: &dyn Function,
        external_env: &mut ExternalEnv,
    ) -> Result<Vec<(&'static str, Option<CompiledArgument>)>, String> {
        let function_arguments = self
            .arguments
            .iter()
            .map(|argument| argument.clone().into_inner())
            .collect::<Vec<_>>();

        // Resolve the arguments so they are in the order defined in the function.
        let arguments = function.resolve_arguments(function_arguments);

        // We take the external context, and pass it to the function compile context, this allows
        // functions mutable access to external state, but keeps the internal compiler state behind
        // an immutable reference, to ensure compiler state correctness.
        let external_context = external_env.swap_external_context(AnyMap::new());

        let mut compile_ctx =
            FunctionCompileContext::new(self.span).with_external_context(external_context);

        let compiled_arguments = arguments
            .iter()
            .map(|(keyword, argument)| -> Result<_, String> {
                let expression = argument.as_ref().map(|argument| &argument.expression);

                // Call `compile_argument` for functions that need to perform any compile time processing
                // on the argument.
                let compiled_argument = function
                    .compile_argument(&arguments, &mut compile_ctx, keyword, expression)
                    .map_err(|error| error.to_string())?;

                let argument = match compiled_argument {
                    Some(argument) => Some(CompiledArgument::Static(argument)),
                    None => argument
                        .as_ref()
                        .map(|argument| CompiledArgument::Dynamic(argument.clone())),
                };

                Ok((*keyword, argument))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Re-insert the external context into the compiler state.
        let _ = external_env.swap_external_context(compile_ctx.into_external_context());

        Ok(compiled_arguments)
    }

    pub fn noop() -> Self {
        let expr = Box::new(Noop) as _;

        Self {
            abort_on_error: false,
            expr,
            maybe_fallible_arguments: false,
            span: Span::default(),
            ident: "noop",
            arguments: Arc::new(Vec::new()),
            function_id: 0,
        }
    }

    pub fn arguments_fmt(&self) -> Vec<String> {
        self.arguments
            .iter()
            .map(|arg| arg.inner().to_string())
            .collect::<Vec<_>>()
    }

    pub fn arguments_dbg(&self) -> Vec<String> {
        self.arguments
            .iter()
            .map(|arg| format!("{:?}", arg.inner()))
            .collect::<Vec<_>>()
    }
}

#[derive(Debug)]
enum CompiledArgument {
    Static(Box<dyn Any + Send + Sync>),
    Dynamic(ResolvedArgument),
}

impl Expression for FunctionCall {
    fn resolve(&self, ctx: &mut Context) -> Resolved {
        self.expr.resolve(ctx).map_err(|err| match err {
            ExpressionError::Abort { .. } => {
                panic!("abort errors must only be defined by `abort` statement")
            }
            ExpressionError::Error {
                message,
                mut labels,
                notes,
            } => {
                labels.push(Label::primary(message.clone(), self.span));

                ExpressionError::Error {
                    message: format!(
                        r#"function call error for "{}" at ({}:{}): {}"#,
                        self.ident,
                        self.span.start(),
                        self.span.end(),
                        message
                    ),
                    labels,
                    notes,
                }
            }
        })
    }

    fn type_def(&self, state: (&LocalEnv, &ExternalEnv)) -> TypeDef {
        let mut type_def = self.expr.type_def(state);

        // If one of the arguments only partially matches the function type
        // definition, then we mark the entire function as fallible.
        //
        // This allows for progressive type-checking, by handling any potential
        // type error the function throws, instead of having to enforce
        // exact-type invariants for individual arguments.
        //
        // That is, this program triggers the `InvalidArgumentKind` error:
        //
        //     slice(10, 1)
        //
        // This is because `slice` expects either a string or an array, but it
        // receives an integer. The concept of "progressive type checking" does
        // not apply in this case, because this call can never succeed.
        //
        // However, given these example events:
        //
        //     { "foo": "bar" }
        //     { "foo": 10.5 }
        //
        // If we were to run the same program, but against the `foo` field:
        //
        //     slice(.foo, 1)
        //
        // In this situation, progressive type checking _does_ make sense,
        // because we can't know at compile-time what the eventual value of
        // `.foo` will be. We mark `.foo` as "any", which includes the "array"
        // and "string" types, so the program can now be made infallible by
        // handling any potential type error the function returns:
        //
        //     slice(.foo, 1) ?? []
        //
        // Note that this rule doesn't just apply to "any" kind (in fact, "any"
        // isn't a kind, it's simply a term meaning "all possible VRL values"),
        // but it applies whenever there's an _intersection_ but not an exact
        // _match_ between two types.
        //
        // Here's another example to demonstrate this:
        //
        //     { "foo": "foobar" }
        //     { "foo": ["foo", "bar"] }
        //     { "foo": 10.5 }
        //
        //     foo = slice(.foo, 1) ?? .foo
        //     .foo = upcase(foo) ?? foo
        //
        // This would result in the following outcomes:
        //
        //     { "foo": "OOBAR" }
        //     { "foo": ["bar", "baz"] }
        //     { "foo": 10.5 }
        //
        // For the first event, both the `slice` and `upcase` functions succeed.
        // For the second event, only the `slice` function succeeds.
        // For the third event, both functions fail.
        //
        if self.maybe_fallible_arguments {
            type_def = type_def.with_fallibility(true);
        }

        if self.abort_on_error {
            type_def = type_def.with_fallibility(false).abortable();
        }

        type_def
    }

    fn compile_to_vm(
        &self,
        vm: &mut crate::vm::Vm,
        (local, external): (&mut LocalEnv, &mut ExternalEnv),
    ) -> Result<(), String> {
        let function = vm
            .function(self.function_id)
            .ok_or(format!("Function {} not found.", self.function_id))?;

        let compiled_arguments = self.compile_arguments(function, external)?;

        for (_, argument) in compiled_arguments {
            match argument {
                Some(CompiledArgument::Static(argument)) => {
                    // The function has compiled this argument as a static.
                    let argument = vm.add_static(argument);
                    vm.write_opcode(OpCode::MoveStaticParameter);
                    vm.write_primitive(argument);
                }
                Some(CompiledArgument::Dynamic(argument)) => {
                    // Compile the argument, `MoveParameter` will move the result of the expression onto the
                    // parameter stack to be passed into the function.
                    argument.expression.compile_to_vm(vm, (local, external))?;
                    vm.write_opcode(OpCode::MoveParameter);
                }
                None => {
                    // The parameter hasn't been specified, so just move an empty parameter onto the
                    // parameter stack.
                    vm.write_opcode(OpCode::EmptyParameter);
                }
            }
        }

        // Call the function with the given id.
        vm.write_opcode(OpCode::Call);
        vm.write_primitive(self.function_id);

        // We need to write the spans for error reporting.
        vm.write_primitive(self.span.start());
        vm.write_primitive(self.span.end());

        Ok(())
    }

    #[cfg(feature = "llvm")]
    fn emit_llvm<'ctx>(
        &self,
        state: (&mut LocalEnv, &mut ExternalEnv),
        ctx: &mut crate::llvm::Context<'ctx>,
    ) -> Result<(), String> {
        let stdlib_function = ctx.stdlib(self.function_id);
        let compiled_arguments = self.compile_arguments(stdlib_function, state.1)?;

        let resolved_type = ctx.result_ref().get_type();

        let function_name = format!("vrl_fn_{}", self.ident);
        let function = ctx
            .module()
            .get_function(&function_name)
            .unwrap_or_else(|| {
                let mut argument_refs = compiled_arguments
                    .iter()
                    .map(|(_, argument)| match argument {
                        Some(CompiledArgument::Static(_)) => ctx.static_ref_type(),
                        Some(CompiledArgument::Dynamic(argument)) if argument.argument.required => {
                            ctx.value_ref_type()
                        }
                        Some(CompiledArgument::Dynamic(_)) | None => ctx.optional_value_ref_type(),
                    })
                    .collect::<Vec<_>>();
                argument_refs.push(resolved_type);
                let argument_refs = argument_refs
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>();
                let function_type = ctx.context().void_type().fn_type(&argument_refs, false);

                ctx.module()
                    .add_function(&function_name, function_type, None)
            });

        let result_ref = ctx.result_ref();

        let mut argument_refs = Vec::new();
        let mut drop_calls = Vec::new();

        for (keyword, argument) in compiled_arguments {
            let argument_name = format!("argument_{}", keyword);
            match argument {
                Some(CompiledArgument::Static(argument)) => {
                    let static_ref = ctx
                        .builder()
                        .build_bitcast(
                            ctx.into_const(argument, &argument_name).as_pointer_value(),
                            ctx.static_ref_type(),
                            "cast",
                        )
                        .into();

                    argument_refs.push(static_ref);
                    drop_calls.push(vec![]);
                }
                Some(CompiledArgument::Dynamic(argument)) if argument.argument.required => {
                    let resolved_ref = ctx.builder().build_alloca(
                        ctx.resolved_ref_type()
                            .get_element_type()
                            .into_struct_type(),
                        &argument_name,
                    );

                    {
                        let fn_ident = "vrl_resolved_initialize";
                        let fn_impl = ctx
                            .module()
                            .get_function(fn_ident)
                            .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
                        ctx.builder()
                            .build_call(fn_impl, &[resolved_ref.into()], fn_ident);
                    }

                    ctx.set_result_ref(resolved_ref);
                    argument.expression.emit_llvm((state.0, state.1), ctx)?;
                    ctx.set_result_ref(result_ref);

                    let value_ref = {
                        let fn_ident = "vrl_resolved_as_value";
                        let fn_impl = ctx
                            .module()
                            .get_function(fn_ident)
                            .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
                        ctx.builder()
                            .build_call(fn_impl, &[resolved_ref.into()], fn_ident)
                            .try_as_basic_value()
                            .left()
                            .ok_or(format!(r#"result of "{}" is not a basic value"#, fn_ident))?
                    };

                    argument_refs.push(value_ref.into());
                    drop_calls.push(vec![(
                        resolved_ref,
                        ctx.module().get_function("vrl_resolved_drop").unwrap(),
                    )]);
                }
                Some(CompiledArgument::Dynamic(argument)) => {
                    let resolved_ref = ctx.builder().build_alloca(
                        ctx.resolved_ref_type()
                            .get_element_type()
                            .into_struct_type(),
                        &argument_name,
                    );
                    let optional_value_ref = ctx.builder().build_alloca(
                        ctx.optional_value_ref_type()
                            .get_element_type()
                            .into_struct_type(),
                        &argument_name,
                    );

                    {
                        let fn_ident = "vrl_resolved_initialize";
                        let fn_impl = ctx
                            .module()
                            .get_function(fn_ident)
                            .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
                        ctx.builder()
                            .build_call(fn_impl, &[resolved_ref.into()], fn_ident);
                    }

                    ctx.set_result_ref(resolved_ref);
                    argument.expression.emit_llvm((state.0, state.1), ctx)?;
                    ctx.set_result_ref(result_ref);

                    {
                        let fn_ident = "vrl_optional_value_initialize";
                        let fn_impl = ctx
                            .module()
                            .get_function(fn_ident)
                            .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
                        ctx.builder()
                            .build_call(fn_impl, &[optional_value_ref.into()], fn_ident);
                    }

                    {
                        let fn_ident = "vrl_resolved_as_value_to_optional_value";
                        let fn_impl = ctx
                            .module()
                            .get_function(fn_ident)
                            .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
                        ctx.builder().build_call(
                            fn_impl,
                            &[resolved_ref.into(), optional_value_ref.into()],
                            fn_ident,
                        );
                    }

                    argument_refs.push(optional_value_ref.into());
                    drop_calls.push(vec![
                        (
                            optional_value_ref,
                            ctx.module()
                                .get_function("vrl_optional_value_drop")
                                .unwrap(),
                        ),
                        (
                            resolved_ref,
                            ctx.module().get_function("vrl_resolved_drop").unwrap(),
                        ),
                    ]);
                }
                None => {
                    let optional_value_ref = ctx.builder().build_alloca(
                        ctx.optional_value_ref_type()
                            .get_element_type()
                            .into_struct_type(),
                        &argument_name,
                    );

                    {
                        let fn_ident = "vrl_optional_value_initialize";
                        let fn_impl = ctx
                            .module()
                            .get_function(fn_ident)
                            .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
                        ctx.builder()
                            .build_call(fn_impl, &[optional_value_ref.into()], fn_ident);
                    }

                    argument_refs.push(optional_value_ref.into());
                    drop_calls.push(vec![(
                        optional_value_ref,
                        ctx.module()
                            .get_function("vrl_optional_value_drop")
                            .unwrap(),
                    )]);
                }
            }
        }

        argument_refs.push(ctx.result_ref().into());

        ctx.builder()
            .build_call(function, &argument_refs, self.ident);

        let type_def = self.expr.type_def((state.0, state.1));
        if type_def.is_fallible() {
            let error = format!(
                r#"function call error for "{}" at ({}:{})"#,
                self.ident,
                self.span.start(),
                self.span.end()
            );

            let error_ref = ctx.into_const(error.clone(), &error).as_pointer_value();

            {
                let fn_ident = "vrl_handle_function_call_result";
                let fn_impl = ctx
                    .module()
                    .get_function(fn_ident)
                    .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
                ctx.builder().build_call(
                    fn_impl,
                    &[
                        ctx.builder()
                            .build_bitcast(
                                error_ref,
                                fn_impl
                                    .get_nth_param(0)
                                    .unwrap()
                                    .get_type()
                                    .into_pointer_type(),
                                "cast",
                            )
                            .into(),
                        result_ref.into(),
                    ],
                    fn_ident,
                );
            }
        }

        argument_refs.pop();

        for drop_calls in drop_calls {
            for (value_ref, drop_fn) in drop_calls {
                let drop_fn_ident = drop_fn.get_name();
                ctx.builder().build_call(
                    drop_fn,
                    &[value_ref.into()],
                    &drop_fn_ident.to_string_lossy(),
                );
            }
        }

        Ok(())
    }
}

impl fmt::Display for FunctionCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.ident.fmt(f)?;
        f.write_str("(")?;

        let arguments = self.arguments_fmt();
        let mut iter = arguments.iter().peekable();
        while let Some(arg) = iter.next() {
            f.write_str(arg)?;

            if iter.peek().is_some() {
                f.write_str(", ")?;
            }
        }

        f.write_str(")")
    }
}

impl fmt::Debug for FunctionCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FunctionCall(")?;
        self.ident.fmt(f)?;

        f.write_str("(")?;

        let arguments = self.arguments_dbg();
        let mut iter = arguments.iter().peekable();
        while let Some(arg) = iter.next() {
            f.write_str(arg)?;

            if iter.peek().is_some() {
                f.write_str(", ")?;
            }
        }

        f.write_str("))")
    }
}

impl PartialEq for FunctionCall {
    fn eq(&self, other: &Self) -> bool {
        self.ident == other.ident
    }
}

// -----------------------------------------------------------------------------

#[derive(thiserror::Error, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error("call to undefined function")]
    Undefined {
        ident_span: Span,
        ident: Ident,
        idents: Vec<&'static str>,
    },

    #[error("wrong number of function arguments")]
    WrongNumberOfArgs { arguments_span: Span, max: usize },

    #[error("unknown function argument keyword")]
    UnknownKeyword {
        keyword_span: Span,
        ident_span: Span,
        keywords: Vec<&'static str>,
    },

    #[error("missing function argument")]
    MissingArgument {
        call_span: Span,
        keyword: &'static str,
        position: usize,
    },

    #[error("function compilation error: error[E{}] {}", error.code(), error)]
    Compilation {
        call_span: Span,
        error: Box<dyn DiagnosticError>,
    },

    #[error("can't abort infallible function")]
    AbortInfallible { ident_span: Span, abort_span: Span },

    #[error("invalid argument type")]
    InvalidArgumentKind {
        function_ident: &'static str,
        abort_on_error: bool,
        arguments_fmt: Vec<String>,
        parameter: Parameter,
        got: Kind,
        argument: FunctionArgument,
        argument_span: Span,
    },

    #[error("fallible argument")]
    FallibleArgument { expr_span: Span },

    #[error("error updating state {}", error)]
    UpdateState { call_span: Span, error: String },
}

impl DiagnosticError for Error {
    fn code(&self) -> usize {
        use Error::*;

        match self {
            Undefined { .. } => 105,
            WrongNumberOfArgs { .. } => 106,
            UnknownKeyword { .. } => 108,
            Compilation { .. } => 610,
            MissingArgument { .. } => 107,
            AbortInfallible { .. } => 620,
            InvalidArgumentKind { .. } => 110,
            FallibleArgument { .. } => 630,
            UpdateState { .. } => 640,
        }
    }

    fn labels(&self) -> Vec<Label> {
        use Error::*;

        match self {
            Undefined {
                ident_span,
                ident,
                idents,
            } => {
                let mut vec = vec![Label::primary("undefined function", ident_span)];
                let ident_chars = ident.as_ref().chars().collect::<Vec<_>>();

                if let Some((idx, _)) = idents
                    .iter()
                    .map(|possible| {
                        let possible_chars = possible.chars().collect::<Vec<_>>();
                        levenstein::distance(&ident_chars, &possible_chars)
                    })
                    .enumerate()
                    .min_by_key(|(_, score)| *score)
                {
                    {
                        let guessed: &str = idents[idx];
                        vec.push(Label::context(
                            format!(r#"did you mean "{}"?"#, guessed),
                            ident_span,
                        ));
                    }
                }

                vec
            }

            WrongNumberOfArgs {
                arguments_span,
                max,
            } => {
                let arg = if *max == 1 { "argument" } else { "arguments" };

                vec![
                    Label::primary("too many function arguments", arguments_span),
                    Label::context(
                        format!("this function takes a maximum of {} {}", max, arg),
                        arguments_span,
                    ),
                ]
            }

            UnknownKeyword {
                keyword_span,
                ident_span,
                keywords,
            } => vec![
                Label::primary("unknown keyword", keyword_span),
                Label::context(
                    format!(
                        "this function accepts the following keywords: {}",
                        keywords
                            .iter()
                            .map(|k| format!(r#""{}""#, k))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    ident_span,
                ),
            ],

            Compilation { call_span, error } => error
                .labels()
                .into_iter()
                .map(|mut label| {
                    label.span = *call_span;
                    label
                })
                .collect(),

            MissingArgument {
                call_span,
                keyword,
                position,
            } => {
                vec![Label::primary(
                    format!(
                        r#"required argument missing: "{}" (position {})"#,
                        keyword, position
                    ),
                    call_span,
                )]
            }

            AbortInfallible {
                ident_span,
                abort_span,
            } => {
                vec![
                    Label::primary("this function can't fail", ident_span),
                    Label::context("remove this abort-instruction", abort_span),
                ]
            }

            InvalidArgumentKind {
                parameter,
                got,
                argument,
                argument_span,
                ..
            } => {
                let keyword = parameter.keyword;
                let expected = parameter.kind();
                let expr_span = argument.span();

                // TODO: extract this out into a helper
                let kind_str = |kind: &Kind| {
                    if kind.is_any() {
                        kind.to_string()
                    } else if kind.is_exact() {
                        format!(r#"the exact type {}"#, kind)
                    } else {
                        format!("one of {}", kind)
                    }
                };

                vec![
                    Label::primary(
                        format!("this expression resolves to {}", kind_str(got)),
                        expr_span,
                    ),
                    Label::context(
                        format!(
                            r#"but the parameter "{}" expects {}"#,
                            keyword,
                            kind_str(&expected)
                        ),
                        argument_span,
                    ),
                ]
            }

            FallibleArgument { expr_span } => vec![
                Label::primary("this expression can fail", expr_span),
                Label::context(
                    "handle the error before passing it in as an argument",
                    expr_span,
                ),
            ],

            UpdateState { call_span, error } => vec![Label::primary(
                format!("an error occurred updating the compiler state: {}", error),
                call_span,
            )],
        }
    }

    fn notes(&self) -> Vec<Note> {
        use Error::*;

        match self {
            WrongNumberOfArgs { .. } => vec![Note::SeeDocs(
                "function arguments".to_owned(),
                Urls::expression_docs_url("#arguments"),
            )],
            AbortInfallible { .. } | FallibleArgument { .. } => vec![Note::SeeErrorDocs],
            InvalidArgumentKind {
                function_ident,
                abort_on_error,
                arguments_fmt,
                parameter,
                argument,
                ..
            } => {
                // TODO: move this into a generic helper function
                let kind = parameter.kind();
                let guard = if kind.is_bytes() {
                    format!("string!({})", argument)
                } else if kind.is_integer() {
                    format!("int!({})", argument)
                } else if kind.is_float() {
                    format!("float!({})", argument)
                } else if kind.is_boolean() {
                    format!("bool!({})", argument)
                } else if kind.is_object() {
                    format!("object!({})", argument)
                } else if kind.is_array() {
                    format!("array!({})", argument)
                } else if kind.is_timestamp() {
                    format!("timestamp!({})", argument)
                } else {
                    return vec![];
                };

                let coerce = if kind.is_bytes() {
                    Some(format!(r#"to_string({}) ?? "default""#, argument))
                } else if kind.is_integer() {
                    Some(format!("to_int({}) ?? 0", argument))
                } else if kind.is_float() {
                    Some(format!("to_float({}) ?? 0", argument))
                } else if kind.is_boolean() {
                    Some(format!("to_bool({}) ?? false", argument))
                } else if kind.is_timestamp() {
                    Some(format!("to_timestamp({}) ?? now()", argument))
                } else {
                    None
                };

                let args = {
                    let mut args = String::new();
                    let mut iter = arguments_fmt.iter().peekable();
                    while let Some(arg) = iter.next() {
                        args.push_str(arg);
                        if iter.peek().is_some() {
                            args.push_str(", ");
                        }
                    }

                    args
                };

                let abort = if *abort_on_error { "!" } else { "" };

                let mut notes = vec![];

                let call = format!("{}{}({})", function_ident, abort, args);

                notes.append(&mut Note::solution(
                    "ensuring an appropriate type at runtime",
                    vec![format!("{} = {}", argument, guard), call.clone()],
                ));

                if let Some(coerce) = coerce {
                    notes.append(&mut Note::solution(
                        "coercing to an appropriate type and specifying a default value as a fallback in case coercion fails",
                        vec![format!("{} = {}", argument, coerce), call],
                    ))
                }

                notes.push(Note::SeeErrorDocs);

                notes
            }

            Compilation { error, .. } => error.notes(),

            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        expression::{Expr, Literal},
        state::ExternalEnv,
        value::kind,
    };

    use super::*;

    #[derive(Clone, Debug)]
    struct Fn;

    impl Expression for Fn {
        fn resolve(&self, _ctx: &mut Context) -> Resolved {
            todo!()
        }

        fn type_def(&self, _state: (&LocalEnv, &ExternalEnv)) -> TypeDef {
            TypeDef::null().infallible()
        }
    }

    #[derive(Debug)]
    struct TestFn;

    impl Function for TestFn {
        fn identifier(&self) -> &'static str {
            "test"
        }

        fn examples(&self) -> &'static [crate::function::Example] {
            &[]
        }

        fn parameters(&self) -> &'static [Parameter] {
            &[
                Parameter {
                    keyword: "one",
                    kind: kind::INTEGER,
                    required: false,
                },
                Parameter {
                    keyword: "two",
                    kind: kind::INTEGER,
                    required: false,
                },
                Parameter {
                    keyword: "three",
                    kind: kind::INTEGER,
                    required: false,
                },
            ]
        }

        fn compile(
            &self,
            _state: (&mut LocalEnv, &mut ExternalEnv),
            _ctx: &mut FunctionCompileContext,
            _arguments: ArgumentList,
        ) -> crate::function::Compiled {
            Ok(Box::new(Fn))
        }

        fn call_by_vm(
            &self,
            _ctx: &mut Context,
            _args: &mut crate::vm::VmArgumentList,
        ) -> Result<value::Value, ExpressionError> {
            unimplemented!()
        }
    }

    fn create_node<T>(inner: T) -> Node<T> {
        Node::new(Span::new(0, 0), inner)
    }

    fn create_argument(ident: Option<&str>, value: i64) -> FunctionArgument {
        FunctionArgument::new(
            ident.map(|ident| create_node(Ident::new(ident))),
            create_node(Expr::Literal(Literal::Integer(value))),
        )
    }

    fn create_resolved_argument(parameter: Parameter, value: i64) -> ResolvedArgument {
        ResolvedArgument {
            argument: parameter,
            expression: Expr::Literal(Literal::Integer(value)),
        }
    }

    fn create_function_call(arguments: Vec<Node<FunctionArgument>>) -> FunctionCall {
        let mut local = LocalEnv::default();
        let mut external = ExternalEnv::default();

        FunctionCall::new(
            Span::new(0, 0),
            Node::new(Span::new(0, 0), Ident::new("test")),
            false,
            arguments,
            &[Box::new(TestFn) as _],
            &mut local,
            &mut external,
        )
        .unwrap()
    }

    #[test]
    fn resolve_arguments_simple() {
        let call = create_function_call(vec![
            create_node(create_argument(None, 1)),
            create_node(create_argument(None, 2)),
            create_node(create_argument(None, 3)),
        ]);

        let parameters = TestFn.parameters();
        let arguments = call.resolve_arguments(&TestFn);
        let expected: Vec<(&'static str, Option<ResolvedArgument>)> = vec![
            ("one", Some(create_resolved_argument(parameters[1], 1))),
            ("two", Some(create_resolved_argument(parameters[2], 2))),
            ("three", Some(create_resolved_argument(parameters[3], 3))),
        ];

        assert_eq!(Ok(expected), arguments);
    }

    #[test]
    fn resolve_arguments_named() {
        let call = create_function_call(vec![
            create_node(create_argument(Some("one"), 1)),
            create_node(create_argument(Some("two"), 2)),
            create_node(create_argument(Some("three"), 3)),
        ]);

        let parameters = TestFn.parameters();
        let arguments = TestFn.resolve_arguments(&TestFn);
        let expected: Vec<(&'static str, Option<ResolvedArgument>)> = vec![
            ("one", Some(create_resolved_argument(parameters[1], 1))),
            ("two", Some(create_resolved_argument(parameters[2], 2))),
            ("three", Some(create_resolved_argument(parameters[3], 3))),
        ];

        assert_eq!(Ok(expected), arguments);
    }

    #[test]
    fn resolve_arguments_named_unordered() {
        let call = create_function_call(vec![
            create_node(create_argument(Some("three"), 3)),
            create_node(create_argument(Some("two"), 2)),
            create_node(create_argument(Some("one"), 1)),
        ]);

        let parameters = TestFn.parameters();
        let arguments = call.resolve_arguments(&TestFn);
        let expected: Vec<(&'static str, Option<ResolvedArgument>)> = vec![
            ("one", Some(create_resolved_argument(parameters[1], 1))),
            ("two", Some(create_resolved_argument(parameters[2], 2))),
            ("three", Some(create_resolved_argument(parameters[3], 3))),
        ];

        assert_eq!(Ok(expected), arguments);
    }

    #[test]
    fn resolve_arguments_unnamed_unordered_one() {
        let call = create_function_call(vec![
            create_node(create_argument(Some("three"), 3)),
            create_node(create_argument(None, 2)),
            create_node(create_argument(Some("one"), 1)),
        ]);

        let parameters = TestFn.parameters();
        let arguments = call.resolve_arguments(&TestFn);
        let expected: Vec<(&'static str, Option<ResolvedArgument>)> = vec![
            ("one", Some(create_resolved_argument(parameters[1], 1))),
            ("two", Some(create_resolved_argument(parameters[2], 2))),
            ("three", Some(create_resolved_argument(parameters[3], 3))),
        ];

        assert_eq!(Ok(expected), arguments);
    }

    #[test]
    fn resolve_arguments_unnamed_unordered_two() {
        let call = create_function_call(vec![
            create_node(create_argument(Some("three"), 3)),
            create_node(create_argument(None, 1)),
            create_node(create_argument(None, 2)),
        ]);

        let parameters = TestFn.parameters();
        let arguments = call.resolve_arguments(&TestFn);
        let expected: Vec<(&'static str, Option<ResolvedArgument>)> = vec![
            ("one", Some(create_resolved_argument(parameters[1], 1))),
            ("two", Some(create_resolved_argument(parameters[2], 2))),
            ("three", Some(create_resolved_argument(parameters[3], 3))),
        ];

        assert_eq!(Ok(expected), arguments);
    }
}
