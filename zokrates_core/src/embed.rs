use crate::solvers::Solver;
use bellman::pairing::ff::ScalarEngine;
use flat_absy::{
    FlatDirective, FlatExpression, FlatExpressionList, FlatFunction, FlatParameter, FlatStatement,
    FlatVariable,
};
use reduce::Reduce;
use std::collections::HashMap;
use typed_absy::types::{FunctionKey, Signature, Type};
use zokrates_embed::{generate_sha256_round_constraints, BellmanConstraint};
use zokrates_field::field::Field;

/// A low level function that contains non-deterministic introduction of variables. It is carried as is until
/// the flattening step when it can be inlined.
#[derive(Debug, Clone, PartialEq)]
pub enum FlatEmbed {
    Sha256Round,
    Unpack,
    CheckU8,
    CheckU16,
    CheckU32,
    U8ToBits,
    U16ToBits,
    U32ToBits,
    U32FromBits,
}

impl FlatEmbed {
    pub fn signature<T: Field>(&self) -> Signature {
        match self {
            FlatEmbed::Sha256Round => Signature::new()
                .inputs(vec![
                    Type::array(Type::FieldElement, 512),
                    Type::array(Type::FieldElement, 256),
                ])
                .outputs(vec![Type::array(Type::FieldElement, 256)]),
            FlatEmbed::Unpack => Signature::new()
                .inputs(vec![Type::FieldElement])
                .outputs(vec![Type::array(
                    Type::FieldElement,
                    T::get_required_bits(),
                )]),
            FlatEmbed::CheckU8 => Signature::new()
                .inputs(vec![Type::Uint(8)])
                .outputs(vec![Type::array(Type::FieldElement, 8)]),
            FlatEmbed::CheckU16 => Signature::new()
                .inputs(vec![Type::Uint(16)])
                .outputs(vec![Type::array(Type::FieldElement, 16)]),
            FlatEmbed::CheckU32 => Signature::new()
                .inputs(vec![Type::Uint(32)])
                .outputs(vec![Type::array(Type::FieldElement, 32)]),
            FlatEmbed::U8ToBits => Signature::new()
                .inputs(vec![Type::Uint(8)])
                .outputs(vec![Type::array(Type::Boolean, 8)]),
            FlatEmbed::U16ToBits => Signature::new()
                .inputs(vec![Type::Uint(16)])
                .outputs(vec![Type::array(Type::Boolean, 16)]),
            FlatEmbed::U32ToBits => Signature::new()
                .inputs(vec![Type::Uint(32)])
                .outputs(vec![Type::array(Type::Boolean, 32)]),
            FlatEmbed::U32FromBits => Signature::new()
                .outputs(vec![Type::Uint(32)])
                .inputs(vec![Type::array(Type::Boolean, 32)]),
        }
    }

    pub fn key<T: Field>(&self) -> FunctionKey<'static> {
        FunctionKey::with_id(self.id()).signature(self.signature::<T>())
    }

    pub fn id(&self) -> &'static str {
        match self {
            FlatEmbed::Sha256Round => "_SHA256_ROUND",
            FlatEmbed::Unpack => "_UNPACK",
            FlatEmbed::CheckU8 => "_CHECK_U8",
            FlatEmbed::CheckU16 => "_CHECK_U16",
            FlatEmbed::CheckU32 => "_CHECK_U32",
            FlatEmbed::U8ToBits => "_U8_TO_BITS",
            FlatEmbed::U16ToBits => "_U16_TO_BITS",
            FlatEmbed::U32ToBits => "_U32_TO_BITS",
            FlatEmbed::U32FromBits => "_U32_FROM_BITS",
        }
    }

    /// Actually get the `FlatFunction` that this `FlatEmbed` represents
    pub fn synthetize<T: Field>(&self) -> FlatFunction<T> {
        match self {
            FlatEmbed::Sha256Round => sha256_round(),
            FlatEmbed::Unpack => unpack_to_host_bitwidth(),
            FlatEmbed::CheckU8 => unpack_to_bitwidth(8),
            FlatEmbed::CheckU16 => unpack_to_bitwidth(16),
            FlatEmbed::CheckU32 => unpack_to_bitwidth(32),
            FlatEmbed::U8ToBits => unreachable!(),
            FlatEmbed::U16ToBits => unreachable!(),
            FlatEmbed::U32ToBits => unreachable!(),
            FlatEmbed::U32FromBits => unreachable!(),
        }
    }
}

// util to convert a vector of `(variable_id, coefficient)` to a flat_expression
fn flat_expression_from_vec<T: Field>(
    v: Vec<(usize, <<T as Field>::BellmanEngine as ScalarEngine>::Fr)>,
) -> FlatExpression<T> {
    match v
        .into_iter()
        .map(|(key, val)| {
            FlatExpression::Mult(
                box FlatExpression::Number(T::from_bellman(val)),
                box FlatExpression::Identifier(FlatVariable::new(key)),
            )
        })
        .reduce(|acc, e| FlatExpression::Add(box acc, box e))
    {
        Some(e @ FlatExpression::Mult(..)) => {
            FlatExpression::Add(box FlatExpression::Number(T::zero()), box e)
        } // the R1CS serializer only recognizes Add
        Some(e) => e,
        None => FlatExpression::Number(T::zero()),
    }
}

impl<T: Field> From<BellmanConstraint<T::BellmanEngine>> for FlatStatement<T> {
    fn from(c: zokrates_embed::BellmanConstraint<T::BellmanEngine>) -> FlatStatement<T> {
        let rhs_a = flat_expression_from_vec(c.a);
        let rhs_b = flat_expression_from_vec(c.b);
        let lhs = flat_expression_from_vec(c.c);

        FlatStatement::Condition(lhs, FlatExpression::Mult(box rhs_a, box rhs_b))
    }
}

/// Returns a flat function which computes a sha256 round
///
/// # Remarks
///
/// The variables inside the function are set in this order:
/// - constraint system variables
/// - arguments
pub fn sha256_round<T: Field>() -> FlatFunction<T> {
    // Define iterators for all indices at hand
    let (r1cs, input_indices, current_hash_indices, output_indices) =
        generate_sha256_round_constraints::<T::BellmanEngine>();

    // indices of the input
    let input_indices = input_indices.into_iter();
    // indices of the current hash
    let current_hash_indices = current_hash_indices.into_iter();
    // indices of the output
    let output_indices = output_indices.into_iter();

    let variable_count = r1cs.aux_count + 1; // auxiliary and ONE

    // indices of the sha256round constraint system variables
    let cs_indices = (0..variable_count).into_iter();

    // indices of the arguments to the function
    // apply an offset of `variable_count` to get the indice of our dummy `input` argument
    let input_argument_indices = input_indices
        .clone()
        .into_iter()
        .map(|i| i + variable_count);
    // apply an offset of `variable_count` to get the indice of our dummy `current_hash` argument
    let current_hash_argument_indices = current_hash_indices
        .clone()
        .into_iter()
        .map(|i| i + variable_count);

    // define parameters to the function based on the variables
    let arguments = input_argument_indices
        .clone()
        .chain(current_hash_argument_indices.clone())
        .map(|i| FlatParameter {
            id: FlatVariable::new(i),
            private: true,
        })
        .collect();

    // define a binding of the first variable in the constraint system to one
    let one_binding_statement = FlatStatement::Condition(
        FlatVariable::new(0).into(),
        FlatExpression::Number(T::from(1)),
    );

    let input_binding_statements =
    // bind input and current_hash to inputs
    input_indices.clone().chain(current_hash_indices).zip(input_argument_indices.clone().chain(current_hash_argument_indices.clone())).map(|(cs_index, argument_index)| {
        FlatStatement::Condition(
            FlatVariable::new(cs_index).into(),
            FlatVariable::new(argument_index).into(),
        )
    });

    // insert flattened statements to represent constraints
    let constraint_statements = r1cs.constraints.into_iter().map(|c| c.into());

    // define which subset of the witness is returned
    let outputs: Vec<FlatExpression<T>> = output_indices
        .map(|o| FlatExpression::Identifier(FlatVariable::new(o)))
        .collect();

    // insert a directive to set the witness based on the bellman gadget and  inputs
    let directive_statement = FlatStatement::Directive(FlatDirective {
        outputs: cs_indices.map(|i| FlatVariable::new(i)).collect(),
        inputs: input_argument_indices
            .chain(current_hash_argument_indices)
            .map(|i| FlatVariable::new(i).into())
            .collect(),
        solver: Solver::Sha256Round,
    });

    // insert a statement to return the subset of the witness
    let return_statement = FlatStatement::Return(FlatExpressionList {
        expressions: outputs,
    });

    let statements = std::iter::once(directive_statement)
        .chain(std::iter::once(one_binding_statement))
        .chain(input_binding_statements)
        .chain(constraint_statements)
        .chain(std::iter::once(return_statement))
        .collect();

    FlatFunction {
        arguments,
        statements,
    }
}

fn use_variable(
    layout: &mut HashMap<String, FlatVariable>,
    name: String,
    index: &mut usize,
) -> FlatVariable {
    let var = FlatVariable::new(*index);
    layout.insert(name, var);
    *index = *index + 1;
    var
}

/// A `FlatFunction` which returns a bit decomposition of a field element
///
/// # Remarks
/// * the return value of the `FlatFunction` is not deterministic: as we decompose over log_2(p) + 1 bits, some
///   elements can have multiple representations: For example, `unpack(0)` is `[0, ..., 0]` but also `unpack(p)`
pub fn unpack_to_host_bitwidth<T: Field>() -> FlatFunction<T> {
    unpack_to_bitwidth(T::get_required_bits())
}

/// A `FlatFunction` which checks a u8
pub fn unpack_to_bitwidth<T: Field>(width: usize) -> FlatFunction<T> {
    let nbits = T::get_required_bits();

    assert!(width <= nbits);

    let mut counter = 0;

    let mut layout = HashMap::new();

    let arguments = vec![FlatParameter {
        id: FlatVariable::new(0),
        private: true,
    }];

    // o0, ..., o253 = ToBits(i0)

    let directive_inputs = vec![FlatExpression::Identifier(use_variable(
        &mut layout,
        format!("i0"),
        &mut counter,
    ))];
    let directive_outputs: Vec<FlatVariable> = (0..width)
        .map(|index| use_variable(&mut layout, format!("o{}", index), &mut counter))
        .collect();

    let solver = Solver::Bits(width);

    let outputs = directive_outputs
        .iter()
        .enumerate()
        .map(|(_, o)| FlatExpression::Identifier(o.clone()))
        .collect::<Vec<_>>();

    // o253, o252, ... o{253 - (width - 1)} are bits
    let mut statements: Vec<FlatStatement<T>> = (0..width)
        .map(|index| {
            let bit = FlatExpression::Identifier(FlatVariable::new(width - index));
            FlatStatement::Condition(
                bit.clone(),
                FlatExpression::Mult(box bit.clone(), box bit.clone()),
            )
        })
        .collect();

    // sum check: o253 + o252 * 2 + ... + o{253 - (width - 1)} * 2**(width - 1)
    let mut lhs_sum = FlatExpression::Number(T::from(0));

    for i in 0..width {
        lhs_sum = FlatExpression::Add(
            box lhs_sum,
            box FlatExpression::Mult(
                box FlatExpression::Identifier(FlatVariable::new(width - i)),
                box FlatExpression::Number(T::from(2).pow(i)),
            ),
        );
    }

    statements.push(FlatStatement::Condition(
        lhs_sum,
        FlatExpression::Mult(
            box FlatExpression::Identifier(FlatVariable::new(0)),
            box FlatExpression::Number(T::from(1)),
        ),
    ));

    statements.insert(
        0,
        FlatStatement::Directive(FlatDirective {
            inputs: directive_inputs,
            outputs: directive_outputs,
            solver: solver,
        }),
    );

    statements.push(FlatStatement::Return(FlatExpressionList {
        expressions: outputs,
    }));

    FlatFunction {
        arguments,
        statements,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zokrates_field::field::FieldPrime;

    #[cfg(test)]
    mod split {
        use super::*;

        #[test]
        fn split254() {
            let unpack: FlatFunction<FieldPrime> = unpack_to_host_bitwidth();

            assert_eq!(
                unpack.arguments,
                vec![FlatParameter::private(FlatVariable::new(0))]
            );
            assert_eq!(
                unpack.statements.len(),
                FieldPrime::get_required_bits() + 1 + 1 + 1
            ); // 128 bit checks, 1 directive, 1 sum check, 1 return
            assert_eq!(
                unpack.statements[0],
                FlatStatement::Directive(FlatDirective::new(
                    (0..FieldPrime::get_required_bits())
                        .map(|i| FlatVariable::new(i + 1))
                        .collect(),
                    Solver::Bits(FieldPrime::get_required_bits()),
                    vec![FlatVariable::new(0)]
                ))
            );
            assert_eq!(
                *unpack.statements.last().unwrap(),
                FlatStatement::Return(FlatExpressionList {
                    expressions: (0..FieldPrime::get_required_bits())
                        .map(|i| FlatExpression::Identifier(FlatVariable::new(i + 1)))
                        .collect()
                })
            );
        }
    }

    #[cfg(test)]
    mod sha256 {
        use super::*;

        #[test]
        fn generate_sha256_constraints() {
            let compiled = sha256_round();

            // function should have 768 inputs
            assert_eq!(compiled.arguments.len(), 768,);

            // function should return 256 values
            assert_eq!(
                compiled
                    .statements
                    .iter()
                    .filter_map(|s| match s {
                        FlatStatement::Return(v) => Some(v),
                        _ => None,
                    })
                    .next()
                    .unwrap()
                    .expressions
                    .len(),
                256,
            );

            // directive should take 768 inputs and return n_var outputs
            let directive = compiled
                .statements
                .iter()
                .filter_map(|s| match s {
                    FlatStatement::Directive(d) => Some(d.clone()),
                    _ => None,
                })
                .next()
                .unwrap();
            assert_eq!(directive.inputs.len(), 768);
            assert_eq!(directive.outputs.len(), 26935);
            // function input should be offset by variable_count
            assert_eq!(
                compiled.arguments[0].id,
                FlatVariable::new(directive.outputs.len() + 1)
            );

            // bellman variable #0: index 0 should equal 1
            assert_eq!(
                compiled.statements[1],
                FlatStatement::Condition(
                    FlatVariable::new(0).into(),
                    FlatExpression::Number(FieldPrime::from(1))
                )
            );

            // bellman input #0: index 1 should equal zokrates input #0: index v_count
            assert_eq!(
                compiled.statements[2],
                FlatStatement::Condition(
                    FlatVariable::new(1).into(),
                    FlatVariable::new(26936).into()
                )
            );

            let f = crate::ir::Function::from(compiled);
            let prog = crate::ir::Prog {
                main: f,
                private: vec![true; 768],
            };

            let input = (0..512)
                .map(|_| FieldPrime::from(0))
                .chain((0..256).map(|_| FieldPrime::from(1)))
                .collect();

            prog.execute(&input).unwrap();
        }
    }
}
