//! Predicate expression plan.

use std::collections::HashMap;

use timely::dataflow::scopes::child::Iterative;
use timely::dataflow::Scope;

pub use crate::binding::{BinaryPredicate as Predicate, BinaryPredicateBinding, Binding};
use crate::plan::{ImplContext, Implementable};
use crate::{Relation, CollectionRelation, Value, Var, VariableMap};

fn lt(a: &Value, b: &Value) -> bool {
    a < b
}
fn lte(a: &Value, b: &Value) -> bool {
    a <= b
}
fn gt(a: &Value, b: &Value) -> bool {
    a > b
}
fn gte(a: &Value, b: &Value) -> bool {
    a >= b
}
fn eq(a: &Value, b: &Value) -> bool {
    a == b
}
fn neq(a: &Value, b: &Value) -> bool {
    a != b
}

/// A plan stage filtering source tuples by the specified
/// predicate. Frontends are responsible for ensuring that the source
/// binds the argument symbols.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Filter<P: Implementable> {
    /// TODO
    pub variables: Vec<Var>,
    /// Logical predicate to apply.
    pub predicate: Predicate,
    /// Plan for the data source.
    pub plan: Box<P>,
    /// Constant intputs
    pub constants: HashMap<u32, Value>,
}

impl<P: Implementable> Implementable for Filter<P> {
    fn dependencies(&self) -> Vec<String> {
        self.plan.dependencies()
    }

    fn into_bindings(&self) -> Vec<Binding> {
        let mut bindings = self.plan.into_bindings();
        let variables = self.variables.clone();

        unimplemented!();
        // bindings.push(Binding::BinaryPredicate(BinaryPredicateBinding {
        //     symbols: (variables[0], variables[1]),
        //     predicate: self.predicate.clone(),
        // }));

        bindings
    }

    fn implement<'b, S: Scope<Timestamp = u64>, I: ImplContext>(
        &self,
        nested: &mut Iterative<'b, S, u64>,
        local_arrangements: &VariableMap<Iterative<'b, S, u64>>,
        context: &mut I,
    ) -> CollectionRelation<'b, S> {
        let rel = self.plan.implement(nested, local_arrangements, context);

        let key_offsets: Vec<usize> = self
            .variables
            .iter()
            .map(|sym| {
                rel.symbols()
                    .iter()
                    .position(|&v| *sym == v)
                    .expect("Symbol not found.")
            })
            .collect();

        let binary_predicate = match self.predicate {
            Predicate::LT => lt,
            Predicate::LTE => lte,
            Predicate::GT => gt,
            Predicate::GTE => gte,
            Predicate::EQ => eq,
            Predicate::NEQ => neq,
        };

        if self.constants.contains_key(&0) {
            let constant = self.constants.get(&0).unwrap().clone();
            CollectionRelation {
                symbols: rel.symbols().to_vec(),
                tuples: rel
                    .tuples()
                    .filter(move |tuple| binary_predicate(&constant, &tuple[key_offsets[0]])),
            }
        } else if self.constants.contains_key(&1) {
            let constant = self.constants.get(&1).unwrap().clone();
            CollectionRelation {
                symbols: rel.symbols().to_vec(),
                tuples: rel
                    .tuples()
                    .filter(move |tuple| binary_predicate(&tuple[key_offsets[0]], &constant)),
            }
        } else {
            CollectionRelation {
                symbols: rel.symbols().to_vec(),
                tuples: rel.tuples().filter(move |tuple| {
                    binary_predicate(&tuple[key_offsets[0]], &tuple[key_offsets[1]])
                }),
            }
        }
    }
}
