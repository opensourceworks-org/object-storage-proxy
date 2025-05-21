// use pyo3::prelude::*;
// use pyo3::types::{IntoPyDict, PyAny, PyFunction};

// pub(crate) fn inspect_callable_signature(py: Python<'_>, callable: &PyAny) -> PyResult<()> {
//     let inspect = py.import("inspect")?;
//     let signature = inspect.call_method1("signature", (callable.into(),))?;
//     let parameters = signature.getattr("parameters")?;

//     for item in parameters.call_method0("items")?.iter()? {
//         let (name, param_obj): (&str, &PyAny) = item?.extract()?;
//         let annotation = param_obj.getattr("annotation")?;

//         println!("Param: {}", name);
//         if annotation.repr()?.to_str()? != "<class 'inspect._empty'>" {
//             println!("  -> Annotation: {}", annotation.repr()?);
//         } else {
//             println!("  -> Annotation: None");
//         }
//     }

//     Ok(())
// }