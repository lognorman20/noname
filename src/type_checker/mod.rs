use std::collections::HashMap;

use crate::{
    error::{Error, ErrorKind, Result},
    imports::FnKind,
    parser::{RootKind, Struct, Ty, TyKind, AST},
};

pub use checker::{FnInfo, StructInfo, TypeChecker};
pub use fn_env::{TypeInfo, TypedFnEnv};

pub mod checker;
pub mod fn_env;

const RESERVED_ARGS: [&str; 1] = ["public_output"];
/// TAST for Typed-AST. Not sure how else to call this,
/// this is to make sure we call this compilation phase before the actual compilation.
#[derive(Debug)]
pub struct TAST {
    pub ast: AST,
    pub typed_global_env: TypeChecker,
}

impl TAST {
    /// This takes the AST produced by the parser, and performs two things:
    /// - resolves imports
    /// - type checks
    pub fn analyze(ast: AST) -> Result<TAST> {
        //
        // inject some utility builtin functions in the scope
        //

        let mut typed_global_env = TypeChecker::default();

        // TODO: should we really import them by default?
        typed_global_env.resolve_global_imports()?;

        //
        // Resolve imports
        //

        for root in &ast.0 {
            match &root.kind {
                // `use crypto::poseidon;`
                RootKind::Use(path) => typed_global_env.import(path)?,
                RootKind::Function(_) => (),
                RootKind::Struct(_) => (),
                RootKind::Comment(_) => (),
                RootKind::Const(_) => (),
            }
        }

        //
        // Type check structs
        //

        for root in &ast.0 {
            match &root.kind {
                // `use crypto::poseidon;`
                RootKind::Struct(struct_) => {
                    let Struct { name, fields, .. } = struct_;

                    let fields: Vec<_> = fields
                        .iter()
                        .map(|field| {
                            let (name, typ) = field;
                            (name.value.clone(), typ.kind.clone())
                        })
                        .collect();

                    let struct_info = StructInfo {
                        name: name.value.clone(),
                        fields,
                        methods: HashMap::new(),
                    };

                    typed_global_env
                        .structs
                        .insert(name.value.clone(), struct_info);
                }

                RootKind::Const(cst) => {
                    typed_global_env.constants.insert(
                        cst.name.value.clone(),
                        Ty {
                            kind: TyKind::Field,
                            span: cst.span,
                        },
                    );
                }

                RootKind::Use(_) | RootKind::Function(_) | RootKind::Comment(_) => (),
            }
        }

        //
        // Semantic analysis includes:
        // - type checking
        // - ?
        //

        for root in &ast.0 {
            match &root.kind {
                // `fn main() { ... }`
                RootKind::Function(function) => {
                    // create a new typed fn environment to type check the function
                    let mut typed_fn_env = TypedFnEnv::default();

                    // if this is main, witness it
                    let is_main = function.is_main();
                    if is_main {
                        typed_global_env.has_main = true;
                    }

                    // save the function in the typed global env
                    let fn_kind = if is_main {
                        FnKind::Main(function.sig.clone())
                    } else {
                        FnKind::Native(function.clone())
                    };
                    let fn_info = FnInfo {
                        kind: fn_kind,
                        span: function.span,
                    };

                    if let Some(self_name) = &function.sig.name.self_name {
                        let struct_info = typed_global_env
                            .structs
                            .get_mut(&self_name.value)
                            .expect("couldn't find the struct for storing the method");

                        struct_info
                            .methods
                            .insert(function.sig.name.name.value.clone(), function.clone());
                    } else {
                        typed_global_env
                            .functions
                            .insert(function.sig.name.name.value.clone(), fn_info);
                    }

                    // store variables and their types in the fn_env
                    for arg in &function.sig.arguments {
                        // public_output is a reserved name,
                        // associated automatically to the public output of the main function
                        if RESERVED_ARGS.contains(&arg.name.value.as_str()) {
                            return Err(Error::new(
                                ErrorKind::PublicOutputReserved(arg.name.value.to_string()),
                                arg.name.span,
                            ));
                        }

                        // `pub` arguments are only for the main function
                        if !is_main && arg.is_public() {
                            return Err(Error::new(
                                ErrorKind::PubArgumentOutsideMain,
                                arg.attribute.as_ref().unwrap().span,
                            ));
                        }

                        // `const` arguments are only for non-main functions
                        if is_main && arg.is_constant() {
                            return Err(Error::new(
                                ErrorKind::ConstArgumentNotForMain,
                                arg.name.span,
                            ));
                        }

                        // store the args' type in the fn environment
                        let arg_typ = arg.typ.kind.clone();

                        if arg.is_constant() {
                            typed_fn_env.store_type(
                                arg.name.value.clone(),
                                TypeInfo::new_cst(arg_typ, arg.span),
                            )?;
                        } else {
                            typed_fn_env.store_type(
                                arg.name.value.clone(),
                                TypeInfo::new(arg_typ, arg.span),
                            )?;
                        }
                    }

                    // the output value returned by the main function is also a main_args with a special name (public_output)
                    if let Some(typ) = &function.sig.return_type {
                        if is_main {
                            if !matches!(typ.kind, TyKind::Field) {
                                unimplemented!();
                            }

                            typed_fn_env.store_type(
                                "public_output".to_string(),
                                TypeInfo::new_mut(typ.kind.clone(), typ.span),
                            )?;
                        }
                    }

                    // type system pass
                    typed_global_env.check_block(
                        &mut typed_fn_env,
                        &function.body,
                        function.sig.return_type.as_ref(),
                    )?;
                }

                RootKind::Use(_)
                | RootKind::Const(_)
                | RootKind::Struct(_)
                | RootKind::Comment(_) => (),
            }
        }

        Ok(TAST {
            ast,
            typed_global_env,
        })
    }
}
