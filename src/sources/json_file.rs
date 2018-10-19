//! Operator and utilities to source data from plain files containing
//! arbitrary json structures.

#[cfg(feature = "json-source")]
extern crate json;
extern crate timely;

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use timely::dataflow::operators::generic;
use timely::dataflow::{Scope, Stream};

use Value;

use sources::Sourceable;

/// A local filesystem data source containing JSON objects.
#[derive(Deserialize, Clone, Debug)]
pub struct JsonFile {
    /// Path to a file on each workers local filesystem.
    pub path: String,
}

#[cfg(feature = "json-source")]
impl Sourceable for JsonFile {
    fn source<G: Scope>(&self, scope: &G) -> Stream<G, (Vec<Value>, u64, isize)> {
        let filename = self.path.clone();

        generic::operator::source(scope, &format!("File({})", filename), |capability| {
            let mut cap = Some(capability);

            let worker_index = scope.index();
            let num_workers = scope.peers();

            move |output| {
                if let Some(cap) = cap.as_mut() {
                    let path = Path::new(&filename);
                    let file = File::open(&path).unwrap();
                    let reader = BufReader::new(file);

                    let mut num_objects_read = 0;
                    let mut object_index = 0;

                    for readline in reader.lines() {
                        let line = readline.ok().expect("read error");

                        if (object_index % num_workers == worker_index) && line.len() > 0 {
                            let obj = json::parse(&line).unwrap();
                            let mut session = output.session(&cap);

                            for (k, v) in obj.entries() {
                                match v {
                                    json::JsonValue::Short(v) => {
                                        session.give((
                                            vec![
                                                Value::Eid(object_index as u64),
                                                Value::String(k.to_string()),
                                                Value::String(v.to_string()),
                                            ],
                                            0,
                                            1,
                                        ));
                                    }
                                    _ => println!("{:?} unsupported, ignoring", v),
                                }
                            }
                            num_objects_read += 1;
                        }

                        object_index += 1;
                    }

                    println!(
                        "[WORKER {}] read {} out of {} objects",
                        worker_index, num_objects_read, object_index
                    );
                }

                cap = None;
            }
        })
    }
}

#[cfg(not(feature = "json-source"))]
impl Sourceable for JsonFile {
    fn source<G: Scope>(&self, scope: &G) -> Stream<G, (Vec<Value>, u64, isize)> {
        panic!("Feature 'json-source' must be enabled to use this.");
    }
}
