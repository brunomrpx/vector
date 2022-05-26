use std::fmt;

use crate::{
    expression::Resolved,
    state::{ExternalEnv, LocalEnv},
    vm::{OpCode, Vm},
    Context, Expression, TypeDef, Value,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Noop;

impl Expression for Noop {
    fn resolve(&self, _: &mut Context) -> Resolved {
        Ok(Value::Null)
    }

    fn type_def(&self, _: (&LocalEnv, &ExternalEnv)) -> TypeDef {
        TypeDef::null().infallible()
    }

    fn compile_to_vm(
        &self,
        vm: &mut Vm,
        _state: (&mut LocalEnv, &mut ExternalEnv),
    ) -> Result<(), String> {
        // Noop just adds a Null to the stack.
        let constant = vm.add_constant(Value::Null);
        vm.write_opcode(OpCode::Constant);
        vm.write_primitive(constant);
        Ok(())
    }

    #[cfg(feature = "llvm")]
    fn emit_llvm<'ctx>(
        &self,
        _: (&mut LocalEnv, &mut ExternalEnv),
        _: &mut crate::llvm::Context<'ctx>,
    ) -> Result<(), String> {
        Ok(())
    }
}

impl fmt::Display for Noop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("null")
    }
}
