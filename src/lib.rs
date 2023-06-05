use pep440_rs::{
    PreRelease, PyVersion, Version as VersionBase, VersionSpecifier, VersionSpecifiers,
    PyRange, Operator,
};
use pep508_rs::{MarkerEnvironment, PyPep508Error, Requirement};

use pubgrub::error::PubGrubError;
use pubgrub::range::Range;
use pubgrub::report::{DefaultStringReporter, Reporter};
use pubgrub::solver::{
    choose_package_with_fewest_versions, resolve, Dependencies, DependencyProvider,
};
use pubgrub::version::Version;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};
use std::borrow::Borrow;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

struct PyDependencyProvider {
    versions: RefCell<HashMap<PyPackage, Vec<PyVersion>>>,
    proxy: Py<PyAny>,
}

//#[derive(Serialize, Deserialize)]
#[derive(Clone, Debug)]
struct PyPackage {
    proxy: Py<PyAny>,
}
impl PartialEq for PyPackage {
    fn eq(&self, other: &Self) -> bool {
        Python::with_gil(|py| {
            let fun = self.proxy.getattr(py, "__eq__").unwrap();
            let res = fun.call1(py, (other.proxy.clone(),)).unwrap();

            let ok: bool = res.extract(py).unwrap();
            ok
        })
    }
}
impl Eq for PyPackage {}
impl Hash for PyPackage {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // self.proxy.hash(state);
        //todo!("PyPackage::hash");
        let hash = Python::with_gil(|py| {
            let fun = self.proxy.getattr(py, "__hash__").unwrap();
            let res = fun.call0(py).unwrap();

            let hash: i64 = res.extract(py).unwrap();
            hash
        });
        state.write_i64(hash);
    }
}
impl fmt::Display for PyPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = Python::with_gil(|py| {
            let repr = self.proxy.getattr(py, "__str__").unwrap();
            let repr: String = repr.call0(py).unwrap().extract(py).unwrap();
            repr
        });
        f.write_str(&repr)
    }
}

impl PyDependencyProvider {
    pub fn available_versions(&self, package: &PyPackage) -> impl Iterator<Item = PyVersion> {
        // eprintln!("XXX available-versions: {package}");
        let versions = match self.versions.borrow().get(package) {
            Some(versions) => {
                    versions.to_owned()
            },
            None => {
                Python::with_gil(|py| {
                    let res = match self
                        .proxy
                        .call_method1(py, "available_versions", (package.proxy.clone(),)) {
                            Ok(res) => res,
                            Err(error) => {
                                let traceback = error.traceback(py).expect("raised exception have a backtrace");
                                eprintln!("exception in available_versions():\n {}{}", traceback.format().unwrap(), error);
                                panic!("{error}");
                            }
                        };
                    let res = res.downcast::<PyList>(py).expect("expected a list");
                    let versions: Vec<_> = res
                        .into_iter()
                        .map(|e| {
                            let ee = e.extract::<&str>().unwrap();
                            PyVersion::parse(ee).unwrap()
                        })
                        .collect();
                    versions
                })
            }
        };
        self.versions.borrow_mut().insert(package.to_owned(), versions.to_owned());
        versions.into_iter()
    }

}

fn version_specifier_to_pubgrub(version_specifier: &PyList) -> Range<PyVersion> {
    let mut full_range: Range<PyVersion> = Range::any();
    for item in version_specifier {
        // eprintln!("item: {item:?}");
        //let item: &str = item.extract().expect("Argl!");

        let (op, version): (&str, &PyAny) = item.extract().unwrap();
        let version = version.extract::<PyVersion>().unwrap();
        let range: Range<PyVersion> = match op {
            "==" => Range::exact(version),
            "<=" => Range::strictly_lower_than(version.bump()),
            ">=" => Range::higher_than(version),
            "<" => Range::strictly_lower_than(version),
            ">" => Range::higher_than(version.bump()),
            "!=" => Range::exact(version).negate(),
            "~=" => {
                let release = &version.0.release;
                let next = match release.len() {
                    0 | 1 => {
                        panic!("bad version");
                    },
                    2 => {
                        PyVersion(VersionBase::from_release(vec![release[0] + 1]))
                    },
                    3 => {
                        PyVersion(VersionBase::from_release(vec![release[0], release[1] + 1]))
                    },
                    _other => {
                        panic!("bad version");
                    }
                };

                Range::between(version, next)
            }
            other => {
                eprintln!("unsupported operator: {other}");
                todo!("other");
            }
        };
        full_range = full_range.intersection(&range);
    }
    full_range
}

impl DependencyProvider<PyPackage, PyVersion> for PyDependencyProvider {
    fn should_cancel(&self) -> Result<(), Box<dyn std::error::Error>> {
        // eprintln!("XXX should-cancel");
        Python::with_gil(|py| match self.proxy.call_method0(py, "should_cancel") {
            Ok(yes) => {
                if yes.is_true(py)? {
                    todo!("how the f.ck return an error");
                }
                Ok(())
            }
            Err(e) => {
                eprintln!("error: {e}");
                todo!("report error")
            }
        })
    }
    fn choose_package_version<T: Borrow<PyPackage>, U: Borrow<Range<PyVersion>>>(
        &self,
        potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<PyVersion>), Box<dyn std::error::Error>> {
        // eprintln!("XXX choose-package-version");
        Ok(choose_package_with_fewest_versions(
            |p| self.available_versions(p),
            potential_packages,
        ))
    }

    fn get_dependencies(
        &self,
        package: &PyPackage,
        version: &PyVersion,
    ) -> Result<Dependencies<PyPackage, PyVersion>, Box<dyn std::error::Error>> {
        // eprintln!("XXX get-dependencies");
        Python::with_gil(|py| {
            let vv = version.clone().into_py(py);
            let res =
                self.proxy
                    .call_method1(py, "get_dependencies", (package.proxy.clone(), vv))?;
            if res.is_none(py) {
                Ok(Dependencies::<PyPackage, PyVersion>::Unknown)
            } else {
                let mut deps: rustc_hash::FxHashMap<PyPackage, Range<PyVersion>> =
                    rustc_hash::FxHashMap::default();
                if let Ok(asdict) = res.downcast::<PyDict>(py) {
                    for (k, v) in asdict {
                        let package = PyPackage { proxy: k.into() };
                        if let Ok(version_specifier) = v.downcast::<PyList>() {
                            let full_range = version_specifier_to_pubgrub(version_specifier);
                            deps.insert(package, full_range);
                        } else if let Ok(url) = v.extract::<&str>() {
                            // eprintln!("TODO: handle urls: {url}")
                            // deps.insert()
                        } else {
                            todo!("raise value error {v}")
                        }
                    }
                } else if let Ok(aslist) = res.downcast::<PyList>(py) {
                    for item in aslist {
                        let item: &PyTuple = item.downcast().unwrap();
                        let (k, v): (&PyAny, &PyAny) = item.extract().unwrap();
                        let package = PyPackage { proxy: k.into() };
                        if let Ok(version_specifier) = v.downcast::<PyList>() {
                            let full_range = version_specifier_to_pubgrub(version_specifier);
                            deps.insert(package, full_range);
                        } else if let Ok(url) = v.extract::<&str>() {
                            // eprintln!("TODO: handle urls: {url}")
                            // deps.insert()
                        } else {
                            todo!("raise value error {v}")
                        }
                    }
                } else {
                    todo!("XXX: get-deps-results {res:?}");
                }
                Ok(Dependencies::Known(deps))
            }
        })
    }
}
/// Formats the sum of two numbers as string.
#[pyfunction]
#[pyo3(name = "resolve")]
fn py_resolve(
    py: Python<'_>,
    dependency_provider: Py<PyAny>,
    package: Py<PyAny>,
    version: &str,
) -> PyResult<Py<PyAny>> {
    let dependency_provider = PyDependencyProvider {
        versions: RefCell::new(HashMap::default()),
        proxy: dependency_provider,
    };
    let package = PyPackage { proxy: package };
    let version = PyVersion::parse(version)?;

    match resolve(&dependency_provider, package, version) {
        Ok(res) => Python::with_gil(|py| {
            let dict = PyDict::new(py);
            for (p, v) in res {
                dict.set_item(p.proxy, v).expect("something went wrong");
            }
            Ok(dict.into())
        }),
        Err(PubGrubError::ErrorRetrievingDependencies {
            package,
            version,
            source,
        }) => {
            if let Some(e) = source.downcast_ref::<PyErr>() {
                Err(e.clone_ref(py)) // we want a backtrace here
            } else {
                eprintln!("failed to retrieve python exception!");
                Err(PyRuntimeError::new_err(
                    "error retrieving dependencies (no exception?)",
                ))
            }
        }
        Err(PubGrubError::NoSolution(mut derivation_tree)) => {
            // derivation_tree.collapse_no_versions();
            let report = DefaultStringReporter::report(&derivation_tree);
            Err(PyRuntimeError::new_err(report))
        }
        Err(PubGrubError::DependencyOnTheEmptySet {
            package,
            version,
            dependent,
        }) => Err(PyRuntimeError::new_err(format!(
            "dependency on the empty set: {package} {version}, {dependent}"
        ))),
        Err(PubGrubError::ErrorChoosingPackageVersion(error)) => {
            if let Some(e) = error.downcast_ref::<PyErr>() {
                Err(e.clone_ref(py)) // we want a backtrace here
            } else {
                eprintln!("failed to retrieve python exception!");
                Err(PyRuntimeError::new_err(
                    "error choosing package version (no exception?)",
                ))
            }
        }
        Err(other) => Err(PyRuntimeError::new_err(format!("other: {other}"))),
    }
}
/// A Python module implemented in Rust.
#[pymodule]
fn _pubgrub(py: Python, m: &PyModule) -> PyResult<()> {
    #[allow(unused_must_use)]
    {
        pyo3_log::try_init();
    }

    m.add_class::<PreRelease>()?;
    m.add_class::<PyRange>()?;
    m.add_class::<PyVersion>()?;
    m.add_class::<Operator>()?;
    m.add_class::<VersionSpecifier>()?;
    m.add_class::<VersionSpecifiers>()?;

    m.add_class::<Requirement>()?;
    m.add_class::<MarkerEnvironment>()?;
    m.add("Pep508Error", py.get_type::<PyPep508Error>())?;

    m.add_function(wrap_pyfunction!(py_resolve, m)?)?;
    Ok(())
}
