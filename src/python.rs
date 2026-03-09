use crate::PyAppState;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::ffi::c_str::CString;
use std::fs;
use std::path::Path;

pub fn load_handler(path: &Path) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let handler = CString::new(fs::read_to_string(path).unwrap()).unwrap();
        let file_name =
            CString::new(path.file_name().unwrap().as_encoded_bytes().to_vec()).unwrap();
        let module_name =
            CString::new(path.file_prefix().unwrap().as_encoded_bytes().to_vec()).unwrap();
        let imported = PyModule::from_code(py, &handler, &file_name, &module_name)?;

        Ok(imported
            .getattr(pyo3::intern!(py, "Handler"))?
            .call0()?
            .unbind())
    })
}

pub fn create_interpreter(state: PyAppState) -> PyResult<()> {
    let interpreter = Python::attach(|py| {
        let locals = PyDict::new(py);
        locals.set_item("state", Py::new(py, state)?)?;
        py.run(
            c"import code; code.interact('', local={\"state\": state})",
            None,
            Some(&locals),
        )
    });

    interpreter
}

#[pymodule]
pub mod wirejack {
    #[pymodule_export]
    use crate::http::{PyHttpRequest, PyHttpResponse};
}
