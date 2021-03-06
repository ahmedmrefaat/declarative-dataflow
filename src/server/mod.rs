//! Server logic for driving the library via commands.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use timely::dataflow::{ProbeHandle, Scope};

use differential_dataflow::collection::Collection;
use differential_dataflow::trace::TraceReader;

use crate::domain::Domain;
use crate::plan::{ImplContext, Implementable};
use crate::sources::{Source, Sourceable};
use crate::Rule;
use crate::{
    implement, implement_neu, AttributeSemantics, CollectionIndex, RelationHandle, TraceKeyHandle,
};
use crate::{Aid, Error, TxData, Value};

/// Server configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// Port at which this server will listen at.
    pub port: u16,
    /// Should inputs via CLI be accepted?
    pub enable_cli: bool,
    /// Should as-of queries be possible?
    pub enable_history: bool,
    /// Should queries use the optimizer during implementation?
    pub enable_optimizer: bool,
    /// Should queries on the query graph be available?
    pub enable_meta: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            port: 6262,
            enable_cli: false,
            enable_history: false,
            enable_optimizer: false,
            enable_meta: false,
        }
    }
}

/// A request expressing interest in receiving results published under
/// the specified name.
#[derive(Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
pub struct Interest {
    /// The name of a previously registered dataflow.
    pub name: String,
}

/// A request with the intent of synthesising one or more new rules
/// and optionally publishing one or more of them.
#[derive(Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
pub struct Register {
    /// A list of rules to synthesise in order.
    pub rules: Vec<Rule>,
    /// The names of rules that should be published.
    pub publish: Vec<String>,
}

/// A request with the intent of attaching to an external data source
/// and publishing it under a globally unique name.
#[derive(Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
pub struct RegisterSource {
    /// One or more globally unique names.
    pub names: Vec<String>,
    /// A source configuration.
    pub source: Source,
}

/// A request with the intent of creating a new named, globally
/// available input that can be transacted upon.
#[derive(Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
pub struct CreateAttribute {
    /// A globally unique name under which to publish data sent via
    /// this input.
    pub name: String,
    /// Semantics enforced on this attribute by 3DF (vs those enforced
    /// by the external source).
    pub semantics: AttributeSemantics,
}

/// Possible request types.
#[derive(Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    /// Sends inputs via one or more registered handles.
    Transact(Vec<TxData>),
    /// Expresses interest in a named relation.
    Interest(Interest),
    /// Registers one or more named relations.
    Register(Register),
    /// Registers an external data source.
    RegisterSource(RegisterSource),
    /// Creates a named input handle that can be `Transact`ed upon.
    CreateAttribute(CreateAttribute),
    /// Advances the specified domain to the specified time.
    AdvanceDomain(Option<String>, u64),
    /// Closes a named input handle.
    CloseInput(String),
}

/// Server context maintaining globally registered arrangements and
/// input handles.
pub struct Server<Token: Hash> {
    /// Server configuration.
    pub config: Config,
    /// Implementation context.
    pub context: Context,
    /// Mapping from query names to interested client tokens.
    pub interests: HashMap<String, Vec<Token>>,
    /// Probe keeping track of overall dataflow progress.
    pub probe: ProbeHandle<u64>,
}

/// Implementation context.
pub struct Context {
    /// Representation of named rules.
    pub rules: HashMap<Aid, Rule>,
    /// Set of rules known to be underconstrained.
    pub underconstrained: HashSet<Aid>,
    /// Internal domain of command sequence numbers.
    pub internal: Domain<u64>,
    /// Named relations.
    pub arrangements: HashMap<Aid, RelationHandle>,
}

impl Context {
    /// Inserts a new named relation.
    pub fn register_arrangement(&mut self, name: String, mut trace: RelationHandle) {
        // decline the capability for that trace handle to subset its
        // view of the data
        trace.distinguish_since(&[]);

        self.arrangements.insert(name, trace);
    }
}

impl ImplContext for Context {
    fn rule(&self, name: &str) -> Option<&Rule> {
        self.rules.get(name)
    }

    fn global_arrangement(&mut self, name: &str) -> Option<&mut RelationHandle> {
        self.arrangements.get_mut(name)
    }

    fn forward_index(&mut self, name: &str) -> Option<&mut CollectionIndex<Value, Value, u64>> {
        self.internal.forward.get_mut(name)
    }

    fn reverse_index(&mut self, name: &str) -> Option<&mut CollectionIndex<Value, Value, u64>> {
        self.internal.reverse.get_mut(name)
    }

    fn is_underconstrained(&self, _name: &str) -> bool {
        // self.underconstrained.contains(name)
        true
    }
}

impl<Token: Hash> Server<Token> {
    /// Creates a new server state from a configuration.
    pub fn new(config: Config) -> Self {
        Server {
            config,
            context: Context {
                rules: HashMap::new(),
                internal: Domain::new(0),
                underconstrained: HashSet::new(),
                arrangements: HashMap::new(),
            },
            interests: HashMap::new(),
            probe: ProbeHandle::new(),
        }
    }

    /// Returns commands to install built-in plans.
    pub fn builtins() -> Vec<Request> {
        vec![
            Request::CreateAttribute(CreateAttribute {
                name: "df.pattern/e".to_string(),
                semantics: AttributeSemantics::Raw,
            }),
            Request::CreateAttribute(CreateAttribute {
                name: "df.pattern/a".to_string(),
                semantics: AttributeSemantics::Raw,
            }),
            Request::CreateAttribute(CreateAttribute {
                name: "df.pattern/v".to_string(),
                semantics: AttributeSemantics::Raw,
            }),
            Request::CreateAttribute(CreateAttribute {
                name: "df.join/binding".to_string(),
                semantics: AttributeSemantics::Raw,
            }),
            Request::CreateAttribute(CreateAttribute {
                name: "df.union/binding".to_string(),
                semantics: AttributeSemantics::Raw,
            }),
            Request::CreateAttribute(CreateAttribute {
                name: "df.project/binding".to_string(),
                semantics: AttributeSemantics::Raw,
            }),
            Request::CreateAttribute(CreateAttribute {
                name: "df.project/symbols".to_string(),
                semantics: AttributeSemantics::Raw,
            }),
            Request::CreateAttribute(CreateAttribute {
                name: "df/name".to_string(),
                semantics: AttributeSemantics::Raw,
            }),
            Request::CreateAttribute(CreateAttribute {
                name: "df.name/symbols".to_string(),
                semantics: AttributeSemantics::Raw,
            }),
            Request::CreateAttribute(CreateAttribute {
                name: "df.name/plan".to_string(),
                semantics: AttributeSemantics::Raw,
            }),
            // Request::Register(Register {
            //     publish: vec!["df.rules".to_string()],
            //     rules: vec![
            //         // [:name {:join/binding [:pattern/e :pattern/a :pattern/v]}]
            //         Rule {
            //             name: "df.rules".to_string(),
            //             plan: Plan::Pull(Pull {
            //                 paths: vec![
            //                     PullLevel {
            //                         variables: vec![],
            //                         plan: Box::new(Plan::MatchA(0, "df.join/binding".to_string(), 1)),
            //                         pull_attributes: vec!["df.pattern/e".to_string(),
            //                                               "df.pattern/a".to_string(),
            //                                               "df.pattern/v".to_string()],
            //                         path_attributes: vec!["df.join/binding".to_string()],
            //                     },
            //                     PullLevel {
            //                         variables: vec![],
            //                         plan: Box::new(Plan::MatchA(0, "df/name".to_string(), 2)),
            //                         pull_attributes: vec![],
            //                         path_attributes: vec![],
            //                     }
            //                 ]
            //             })
            //         }
            //     ],
            // }),
        ]
    }

    /// Handle a Transact request.
    pub fn transact(
        &mut self,
        tx_data: Vec<TxData>,
        owner: usize,
        worker_index: usize,
    ) -> Result<(), Error> {
        // only the owner should actually introduce new inputs
        if owner == worker_index {
            self.context.internal.transact(tx_data)
        } else {
            Ok(())
        }
    }

    /// Handles an Interest request.
    pub fn interest<S: Scope<Timestamp = u64>>(
        &mut self,
        name: &str,
        scope: &mut S,
    ) -> Result<&mut TraceKeyHandle<Vec<Value>, u64, isize>, Error> {
        match name {
            "df.timely/operates" => {
                // use timely::logging::{BatchLogger, TimelyEvent};
                // use timely::dataflow::operators::capture::EventWriter;

                // let writer = EventWriter::new(stream);
                // let mut logger = BatchLogger::new(writer);
                // scope.log_register()
                //     .insert::<TimelyEvent,_>("timely", move |time, data| logger.publish_batch(time, data));

                // logging_stream
                //     .flat_map(|(t,_,x)| {
                //         if let Operates(event) = x {
                //             Some((event, t, 1 as isize))
                //         } else { None }
                //     })
                //     .as_collection()

                unimplemented!();
            }
            _ => {
                // We need to do a `contains_key` here to avoid taking
                // a mut ref on context.
                if self.context.arrangements.contains_key(name) {
                    // Rule is already implemented.
                    Ok(self.context.global_arrangement(name).unwrap())
                } else if self.config.enable_optimizer {
                    let rel_map = implement_neu(name, scope, &mut self.context)?;

                    for (name, trace) in rel_map.into_iter() {
                        self.context.register_arrangement(name, trace);
                    }

                    match self.context.global_arrangement(name) {
                        None => Err(Error {
                            category: "df.error.category/fault",
                            message: format!(
                                "Relation of interest ({}) wasn't actually implemented.",
                                name
                            ),
                        }),
                        Some(trace) => Ok(trace),
                    }
                } else {
                    let rel_map = implement(name, scope, &mut self.context)?;

                    for (name, trace) in rel_map.into_iter() {
                        self.context.register_arrangement(name, trace);
                    }

                    match self.context.global_arrangement(name) {
                        None => Err(Error {
                            category: "df.error.category/fault",
                            message: format!(
                                "Relation of interest ({}) wasn't actually implemented.",
                                name
                            ),
                        }),
                        Some(trace) => Ok(trace),
                    }
                }
            }
        }
    }

    /// Handle a Register request.
    pub fn register(&mut self, req: Register) -> Result<(), Error> {
        let Register { rules, .. } = req;

        for rule in rules.into_iter() {
            if self.context.rules.contains_key(&rule.name) {
                // @TODO panic if hashes don't match
                // panic!("Attempted to re-register a named relation");
                continue;
            } else {
                if self.config.enable_meta {
                    let mut data = rule.plan.datafy();
                    let tx_data: Vec<TxData> =
                        data.drain(..).map(|(e, a, v)| TxData(1, e, a, v)).collect();

                    self.transact(tx_data, 0, 0)?;
                }

                self.context.rules.insert(rule.name.to_string(), rule);
            }
        }

        Ok(())
    }

    /// Handle a RegisterSource request.
    pub fn register_source<S: Scope<Timestamp = u64>>(
        &mut self,
        req: RegisterSource,
        scope: &mut S,
    ) -> Result<(), Error> {
        let RegisterSource { mut names, source } = req;

        if names.len() == 1 {
            let name = names.pop().unwrap();
            let datoms = source.source(scope, names.clone());

            self.context.internal.create_source(&name, None, &datoms)
        } else if names.len() > 1 {
            let datoms = source.source(scope, names.clone());

            for (name_idx, name) in names.iter().enumerate() {
                self.context
                    .internal
                    .create_source(name, Some(name_idx), &datoms)?;
            }

            Ok(())
        } else {
            Ok(())
        }
    }

    /// Handle an AdvanceDomain request.
    pub fn advance_domain(&mut self, name: Option<String>, next: u64) -> Result<(), Error> {
        match name {
            None => {
                // If history is not enabled, we want to keep traces advanced
                // up to the previous time.
                let trace_next = if self.config.enable_history {
                    None
                } else {
                    Some(next - 1)
                };

                self.context.internal.advance_to(next, trace_next);

                if let Some(trace_next) = trace_next {
                    // if historical queries don't matter, we should advance
                    // the index traces to allow them to compact

                    let frontier = &[trace_next];

                    for trace in self.context.arrangements.values_mut() {
                        trace.advance_by(frontier);
                    }
                }

                Ok(())
            }
            Some(_) => Err(Error {
                category: "df.error.category/unsupported",
                message: "Named domains are not yet supported.".to_string(),
            }),
        }
    }

    /// Returns true iff the probe is behind any input handle. Mostly
    /// used as a convenience method during testing.
    pub fn is_any_outdated(&self) -> bool {
        if self.probe.less_than(self.context.internal.time()) {
            return true;
        }

        false
    }

    /// Helper for registering, publishing, and indicating interest in
    /// a single, named query. Used for testing.
    pub fn test_single<S: Scope<Timestamp = u64>>(
        &mut self,
        scope: &mut S,
        rule: Rule,
    ) -> Collection<S, Vec<Value>, isize> {
        let interest_name = rule.name.clone();
        let publish_name = rule.name.clone();

        self.register(Register {
            rules: vec![rule],
            publish: vec![publish_name],
        })
        .unwrap();

        match self.interest(&interest_name, scope) {
            Err(error) => panic!("{:?}", error),
            Ok(trace) => trace
                .import_named(scope, &interest_name)
                .as_collection(|tuple, _| tuple.clone())
                .probe_with(&mut self.probe),
        }
    }
}
