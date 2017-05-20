#[macro_use]
extern crate cpython;
extern crate hyper;

use cpython::{Python, PyResult, PyObject, PyList, PyString};
use cpython::PythonObject;
use hyper::server::Server;

mod handler;
use handler::WSGIHandler;

fn main() {
    // TODO: parse command line options
    let app: PyObject;
    {
        let gil = Python::acquire_gil();
        app = load_application(gil.python()).unwrap();
    }
    let addr = "127.0.0.1:9000";
    println!("listening on {}", addr);
    let mut server = Server::http(addr).unwrap();
    // disable keep-alive as it cause some hung-up time while browsing directory
    server.keep_alive(None);
    server.handle(WSGIHandler{app}).unwrap();
}

fn load_application(py: Python) -> PyResult<PyObject> {
    // to import wsgi module from current directory, add it to sys.path
    let current_dir = std::env::current_dir().unwrap();
    let sys = py.import("sys")?;
    let sys_path: PyList = sys.get(py, "path")?.extract(py)?;
    sys_path.insert_item(py, 0, PyString::new(py, current_dir.to_str().unwrap()).into_object());

    let app_name = std::env::args().nth(1).unwrap_or("wsgi".to_string());
    let module = py.import(app_name.as_str())?;
    let app = module.get(py, "application")?;
    println!("application: {:?}", app);
    Ok(app)
}
