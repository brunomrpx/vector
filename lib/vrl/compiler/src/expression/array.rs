use std::{collections::BTreeMap, fmt, ops::Deref};

use crate::{
    expression::{Expr, Resolved},
    state::{ExternalEnv, LocalEnv},
    vm::OpCode,
    Context, Expression, TypeDef, Value,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Array {
    inner: Vec<Expr>,
}

impl Array {
    pub(crate) fn new(inner: Vec<Expr>) -> Self {
        Self { inner }
    }
}

impl Deref for Array {
    type Target = Vec<Expr>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Expression for Array {
    fn resolve(&self, ctx: &mut Context) -> Resolved {
        self.inner
            .iter()
            .map(|expr| expr.resolve(ctx))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array)
    }

    fn as_value(&self) -> Option<Value> {
        self.inner
            .iter()
            .map(|expr| expr.as_value())
            .collect::<Option<Vec<_>>>()
            .map(Value::Array)
    }

    fn type_def(&self, state: (&LocalEnv, &ExternalEnv)) -> TypeDef {
        let type_defs = self
            .inner
            .iter()
            .map(|expr| expr.type_def(state))
            .collect::<Vec<_>>();

        // If any of the stored expressions is fallible, the entire array is
        // fallible.
        let fallible = type_defs.iter().any(TypeDef::is_fallible);

        let collection = type_defs
            .into_iter()
            .enumerate()
            .map(|(index, type_def)| (index.into(), type_def.into()))
            .collect::<BTreeMap<_, _>>();

        TypeDef::array(collection).with_fallibility(fallible)
    }

    fn compile_to_vm(
        &self,
        vm: &mut crate::vm::Vm,
        state: (&mut LocalEnv, &mut ExternalEnv),
    ) -> Result<(), String> {
        let (local, external) = state;

        // Evaluate each of the elements of the array, the result of each
        // will be added to the stack.
        for value in self.inner.iter().rev() {
            value.compile_to_vm(vm, (local, external))?;
        }

        vm.write_opcode(OpCode::CreateArray);

        // Add the length of the array as a primitive so the VM knows how
        // many elements to move into the array.
        vm.write_primitive(self.inner.len());

        Ok(())
    }

    #[cfg(feature = "llvm")]
    fn emit_llvm<'ctx>(
        &self,
        state: (&mut LocalEnv, &mut ExternalEnv),
        ctx: &mut crate::llvm::Context<'ctx>,
    ) -> Result<(), String> {
        let function = ctx.function();
        let begin_block = ctx.context().append_basic_block(function, "array_begin");
        ctx.builder().build_unconditional_branch(begin_block);
        ctx.builder().position_at_end(begin_block);

        let end_block = ctx.context().append_basic_block(function, "array_end");

        let vec_ref = ctx.builder().build_alloca(ctx.vec_type(), "temp");

        {
            let fn_ident = "vrl_vec_initialize";
            let fn_impl = ctx
                .module()
                .get_function(fn_ident)
                .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
            ctx.builder().build_call(
                fn_impl,
                &[
                    vec_ref.into(),
                    ctx.usize_type()
                        .const_int(self.inner.len() as _, false)
                        .into(),
                ],
                fn_ident,
            )
        };

        let insert_block = ctx.context().append_basic_block(function, "array_insert");
        ctx.builder().build_unconditional_branch(insert_block);
        ctx.builder().position_at_end(insert_block);

        for (index, value) in self.inner.iter().enumerate() {
            let type_def = value.type_def((state.0, state.1));
            if type_def.is_abortable() {
                let is_err = {
                    let fn_ident = "vrl_resolved_is_err";
                    let fn_impl = ctx
                        .module()
                        .get_function(fn_ident)
                        .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
                    ctx.builder()
                        .build_call(fn_impl, &[ctx.result_ref().into()], fn_ident)
                        .try_as_basic_value()
                        .left()
                        .ok_or(format!(r#"result of "{}" is not a basic value"#, fn_ident))?
                        .try_into()
                        .map_err(|_| format!(r#"result of "{}" is not an int value"#, fn_ident))?
                };

                let insert_block = ctx.context().append_basic_block(function, "array_insert");
                ctx.builder()
                    .build_conditional_branch(is_err, end_block, insert_block);
                ctx.builder().position_at_end(insert_block);
            }

            {
                let fn_ident = "vrl_vec_insert";
                let fn_impl = ctx
                    .module()
                    .get_function(fn_ident)
                    .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
                ctx.builder().build_call(
                    fn_impl,
                    &[
                        vec_ref.into(),
                        ctx.usize_type().const_int(index as _, false).into(),
                        ctx.result_ref().into(),
                    ],
                    fn_ident,
                )
            };
        }

        let set_result_block = ctx
            .context()
            .append_basic_block(function, "array_set_result");
        ctx.builder().build_unconditional_branch(set_result_block);
        ctx.builder().position_at_end(set_result_block);

        {
            let fn_ident = "vrl_expression_array_set_result_impl";
            let fn_impl = ctx
                .module()
                .get_function(fn_ident)
                .ok_or(format!(r#"failed to get "{}" function"#, fn_ident))?;
            ctx.builder().build_call(
                fn_impl,
                &[vec_ref.into(), ctx.result_ref().into()],
                fn_ident,
            )
        };

        ctx.builder().build_unconditional_branch(end_block);
        ctx.builder().position_at_end(end_block);

        Ok(())
    }
}

impl fmt::Display for Array {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let exprs = self
            .inner
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");

        write!(f, "[{}]", exprs)
    }
}

impl From<Vec<Expr>> for Array {
    fn from(inner: Vec<Expr>) -> Self {
        Self { inner }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{expr, test_type_def, value::Kind, TypeDef};
    use value::kind::Collection;

    test_type_def![
        empty_array {
            expr: |_| expr!([]),
            want: TypeDef::array(Collection::empty()),
        }

        scalar_array {
            expr: |_| expr!([1, "foo", true]),
            want: TypeDef::array(BTreeMap::from([
                (0.into(), Kind::integer()),
                (1.into(), Kind::bytes()),
                (2.into(), Kind::boolean()),
            ])),
        }

        mixed_array {
            expr: |_| expr!([1, [true, "foo"], { "bar": null }]),
            want: TypeDef::array(BTreeMap::from([
                (0.into(), Kind::integer()),
                (1.into(), Kind::array(BTreeMap::from([
                    (0.into(), Kind::boolean()),
                    (1.into(), Kind::bytes()),
                ]))),
                (2.into(), Kind::object(BTreeMap::from([
                    ("bar".into(), Kind::null())
                ]))),
            ])),
        }
    ];
}
