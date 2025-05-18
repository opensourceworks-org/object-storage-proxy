use pyo3::prelude::*;
use pyo3::types::{IntoPyDict, PyAny};

fn inspect_callable_signature(py: Python<'_>, callable: &PyAny) -> PyResult<()> {
    let inspect = py.import("inspect")?;
    let signature = inspect.call_method1("signature", (callable,))?;
    let parameters = signature.getattr("parameters")?;

    for (name, param) in parameters.iter()? {
        let name: &str = name?.extract()?;
        let param_obj = param?;
        let annotation = param_obj.getattr("annotation")?;

        println!("Param: {}", name);
        if annotation.repr()?.to_str()? != "<class 'inspect._empty'>" {
            println!("  -> Annotation: {}", annotation.repr()?);
        } else {
            println!("  -> Annotation: None");
        }
    }

    Ok(())
}