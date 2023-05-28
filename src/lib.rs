use pep440_rs::{PyVersion, Version as VersionBase, VersionSpecifiers, VersionSpecifier};
use pep508_rs::modern::VersionSpecifierModern;
use pep508_rs::{MarkerEnvironment, Requirement};

use pubgrub::error::PubGrubError;
use pubgrub::range::Range;
use pubgrub::solver::{
    choose_package_with_fewest_versions, resolve, Dependencies, DependencyProvider,
};
use pubgrub::version::Version;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};
use std::borrow::Borrow;

use std::fmt;
use std::hash::Hash;

struct PyDependencyProvider {
    proxy: Py<PyAny>,
}

//#[derive(Serialize, Deserialize)]
#[pyclass(subclass)]
struct PyRange(Range<PyVersion>);
#[pymethods]
impl PyRange {
    fn __str__(&self) -> String {
        self.0.to_string()
    }

    fn __repr__(&self) -> String {
        let s = self.0.to_string();
        format!("Range('{s}')")
    }
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
            let repr = self.proxy.getattr(py, "__repr__").unwrap();
            let repr: String = repr.call0(py).unwrap().extract(py).unwrap();
            repr
        });
        f.write_str(&format!("Rusty({repr})"))
    }
}

// #[derive(Clone, Debug)]
// struct PyVersion {
//     proxy: Py<PyAny>,
// }
// impl fmt::Display for PyVersion {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         let repr = Python::with_gil(|py| {
//             let repr = self.proxy.getattr(py, "__repr__").unwrap();
//             let repr: String = repr.call0(py).unwrap().extract(py).unwrap();
//             repr
//         });
//         f.write_str(&format!("Rusty({repr})"))
//     }
// }
// impl PartialEq for PyVersion {
//     fn eq(&self, other: &Self) -> bool {
//         Python::with_gil(|py| {
//             let eq = self.proxy.getattr(py, "__eq__").unwrap();
//             let eq: bool = eq
//                 .call1(py, (other.proxy.clone(),))
//                 .unwrap()
//                 .extract(py)
//                 .unwrap();
//             eq
//         })
//     }
// }
// impl Eq for PyVersion {}
// impl PartialOrd for PyVersion {
//     fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
//         // self.proxy.partial_cmp(&other.prox
//         // eprintln!("partial_cmp: {self} {other}");

//         Python::with_gil(|py| {
//             for (attr, cmp) in [
//                 ("__eq__", std::cmp::Ordering::Equal),
//                 ("__gt__", std::cmp::Ordering::Greater),
//                 ("__lt__", std::cmp::Ordering::Less),
//             ] {
//                 if let Ok(ok) = self
//                     .proxy
//                     .call_method1(py, attr, (other.proxy.clone(),))
//                     .unwrap_or_else(|e| {
//                         eprintln!("Got an error! {attr}: {e}");
//                         py.None()
//                     })
//                     .extract(py)
//                 {
//                     if ok {
//                         return Some(cmp);
//                     }
//                 }
//             }
//             None
//         })
//     }
// }
// impl Ord for PyVersion {
//     fn cmp(&self, other: &Self) -> std::cmp::Ordering {
//         //  eprintln!("PyVersion::cmp {self} {other}");
//         self.partial_cmp(other).unwrap()
//     }
// }

impl PyDependencyProvider {
    pub fn available_versions(&self, package: &PyPackage) -> impl Iterator<Item = PyVersion> {
        let versions = Python::with_gil(|py| {
            let fun = self.proxy.getattr(py, "available_versions").unwrap();
            let res = fun.call1(py, (package.proxy.clone(),)).unwrap();
            let res = res.downcast::<PyList>(py).expect("expected a list");
            let versions: Vec<_> = res
                .into_iter()
                .map(|e| {
                    let ee = e.extract::<&str>().unwrap();
                    PyVersion::parse(ee).unwrap()
                })
                .collect();
            versions
        });
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
            "!=" => {
                let b = Range::higher_than(version.bump());
                let a = Range::strictly_lower_than(version);
                a.union(&b)
            }
            "~=" => {
                let release = &version.0.release;
                let vnext  = PyVersion(VersionBase::from_release(vec![release[0], release[1] + 1]));

                Range::between(version, vnext)
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
        mut potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<PyVersion>), Box<dyn std::error::Error>> {
        Ok(pubgrub::solver::choose_package_with_fewest_versions(
            |p| self.available_versions(p),
            potential_packages,
        ))
    }

    //     choose_package_with_fewest_versions
    //     let (package, range) = potential_packages.next().unwrap();

    //     let version = self
    //         .available_versions(package.borrow())
    //         .find(|v| range.borrow().contains(v));
    //     Ok((package, version))
    // }

    fn get_dependencies(
        &self,
        package: &PyPackage,
        version: &PyVersion,
    ) -> Result<Dependencies<PyPackage, PyVersion>, Box<dyn std::error::Error>> {
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
                        let version_specifier: &PyList = v.downcast().unwrap();
                        let full_range = version_specifier_to_pubgrub(version_specifier);
                        deps.insert(package, full_range);
                    }
                } else if let Ok(aslist) = res.downcast::<PyList>(py) {
                    for item in aslist {
                        let item: &PyTuple = item.downcast().unwrap();
                        let (k, v): (&PyAny, &PyAny) = item.extract().unwrap();
                        let package = PyPackage { proxy: k.into() };
                        let version_specifier: &PyList = v.downcast().unwrap();
                        let full_range = version_specifier_to_pubgrub(version_specifier);
                        deps.insert(package, full_range);
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
        Err(error) => {
            if let PubGrubError::ErrorRetrievingDependencies {
                package: _,
                version: _,
                ref source,
            } = error
            {
                if let Some(e) = source.downcast_ref::<PyErr>() {
                    return Err(e.clone_ref(py));
                };
            };
            Err(PyRuntimeError::new_err(format!("error: {error:#?}")))
        }
    }
}
/// A Python module implemented in Rust.
#[pymodule]
fn _pubgrub(_py: Python, m: &PyModule) -> PyResult<()> {
    #[allow(unused_must_user)]
    {
        pyo3_log::try_init();
    }
    m.add_class::<MarkerEnvironment>()?;
    m.add_class::<VersionSpecifiers>()?;
    m.add_class::<VersionSpecifier>()?;
    // m.add_class::<VersionSpecifierModern>()?;
    m.add_class::<Requirement>()?;
    m.add_class::<PyVersion>()?;
    m.add_function(wrap_pyfunction!(py_resolve, m)?)?;

    Ok(())
}
