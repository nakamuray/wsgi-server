use std::cell::RefCell;
use std::io;
use std::io::{Read, Write};
use cpython::{Python, PyClone, PyResult, PyObject, PyBytes, PyDict};
use cpython::{ObjectProtocol, PythonObject};
use hyper::header::{ContentLength, ContentType};
use hyper::server::{Handler, Request, Response};
use hyper::status::StatusCode;
use hyper::uri::RequestUri::AbsolutePath;


py_class!(class WSGIInput |py| {
// TODO: wrap Request, don't read all input onto memory (but how?)
    data buffer: RefCell<Vec<u8>>;
    def read(&self, size: usize) -> PyResult<PyBytes> {
        let mut buffer = self.buffer(py).borrow_mut();
        let size = if size > buffer.len() {
            buffer.len()
        } else {
            size
        };
        let data : Vec<u8> = buffer.drain(..size).collect();
        Ok(PyBytes::new(py, data.as_slice()))
    }
// XXX: is these methods expected to include newline, or not?
    def readline(&self) -> PyResult<PyBytes> {
// TODO: can I avoid cloning and modify buffer in-place?
        let buffer = self.buffer(py).borrow().clone();
        let mut it = buffer.splitn(2, |c| *c == b'\n');
        let head = it.next().unwrap();
        let tail = it.next().unwrap_or(&[]);
        *self.buffer(py).borrow_mut() = tail.to_vec();
        Ok(PyBytes::new(py, head))
    }
    def readlines(&self, _hint: u64) -> PyResult<Vec<PyBytes>> {
        let buffer = self.buffer(py).borrow().clone();
        let lines = buffer.split(|c| *c == b'\n').map(|s| PyBytes::new(py, s)).collect();
        *self.buffer(py).borrow_mut() = vec![];
        Ok(lines)
    }
    def __iter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }
    def __next__(&self) -> PyResult<Option<PyBytes>> {
        if self.buffer(py).borrow().len() == 0 {
            Ok(None)
        } else {
            Ok(Some(self.readline(py)?))
        }
    }
});

py_class!(class WSGIError |py| {
    def flush(&self) -> PyResult<PyObject> {
        io::stderr().flush().unwrap();
        Ok(py.None())
    }
    def write(&self, data: String) -> PyResult<PyObject> {
        io::stderr().write(data.as_bytes()).unwrap();
        Ok(py.None())
    }
    def writelines(&self, data: Vec<String>) -> PyResult<PyObject> {
        for line in data.iter() {
            io::stderr().write(line.as_bytes()).unwrap();
        }
        Ok(py.None())
    }
});

pub struct WSGIHandler {
    pub app: PyObject,
}

impl Handler for WSGIHandler {
    fn handle(&self, mut req: Request, mut res: Response) {
        println!("{:?}", req.headers);
        let gil = Python::acquire_gil();
        let py = gil.python();

        let environ = PyDict::new(py).into_object();
        environ.set_item(py, "REQUEST_METHOD", req.method.to_string()).unwrap();
        environ.set_item(py, "SCRIPT_NAME", "").unwrap();
        if let AbsolutePath(path) = req.uri.clone() {
            let mut it = path.splitn(2, '?');
            let path = it.next().unwrap();
            let query = it.next().unwrap_or("");
            environ.set_item(py, "PATH_INFO", path).unwrap();
            environ.set_item(py, "QUERY_STRING", query).unwrap();
        } else {
            // TODO: return 400(?) bad request
            panic!("bad request");
        }
        // TODO: set correct server name, port
        environ.set_item(py, "SERVER_NAME", "127.0.0.1").unwrap();
        environ.set_item(py, "SERVER_PORT", "9000").unwrap();
        environ.set_item(py, "SERVER_PROTOCOL", req.version.to_string()).unwrap();

        for h in req.headers.iter() {
            if h.is::<ContentType>() {
                environ.set_item(py, "CONTENT_TYPE", h.value_string()).unwrap();
            } else if h.is::<ContentLength>() {
                environ.set_item(py, "CONTENT_LENGTH", h.value_string()).unwrap();
            } else {
                // TODO: concatenate header values if it already set
                let k = ("HTTP_".to_string() + h.name()).replace("-", "_").to_uppercase();
                environ.set_item(py, k, h.value_string()).unwrap();
            }
        }

        let mut buffer = vec![];
        req.read_to_end(&mut buffer).unwrap();

        let input = WSGIInput::create_instance(py, RefCell::new(buffer)).unwrap();
        let error = WSGIError::create_instance(py).unwrap();

        environ.set_item(py, "wsgi.version", (1, 0)).unwrap();
        environ.set_item(py, "wsgi.url_scheme", "http").unwrap();
        environ.set_item(py, "wsgi.input", input).unwrap();
        environ.set_item(py, "wsgi.errors", error).unwrap();
        environ.set_item(py, "wsgi.multithread", true).unwrap();
        environ.set_item(py, "wsgi.multiprocess", false).unwrap();
        environ.set_item(py, "wsgi.run_once", false).unwrap();

        let status = RefCell::new("".to_string());
        let headers = RefCell::new(vec![]);
        // TODO: return 500 if not Ok
        let start_response = StartResponse::create_instance(py, status, headers).unwrap();

        let args = (environ, &start_response);

        let body = match self.app.call(py, args, None) {
            // TODO: make this 500 returning process into macro
            Ok(body) => body,
            Err(e) => {
                println!("{:?}", e);
                *res.status_mut() = StatusCode::InternalServerError;
                let mut res = res.start().unwrap();
                res.write_all(b"Internal Server Error\n").unwrap();
                return ();
            }
        };

        println!("status: {:?}", start_response.status(py).borrow());
        println!("headers: {:?}", start_response.headers(py).borrow());
        println!("body: {:?}", body);

        // TODO: return 500 if not Ok
        let status = start_response.status(py).borrow();
        let str_code = status.split(' ').next().unwrap();
        let code: u16 = str_code.parse().unwrap();
        *res.status_mut() = StatusCode::from_u16(code);

        for h in start_response.headers(py).borrow().iter() {
            let (ref name, ref value) = *h;
            res.headers_mut().append_raw(name.clone().to_owned(), value.clone().into_bytes());
        }

        let mut res = res.start().unwrap();

        // TODO: return 500(?) if not iterable
        for robj in body.iter(py).unwrap() {
            let bytes_obj: PyBytes = robj.unwrap().extract(py).unwrap();
            let chunk = bytes_obj.data(py);
            res.write_all(chunk).unwrap();
        }
        res.end().unwrap();
    }
}

py_class!(class StartResponse |py| {
    data status: RefCell<String>;
    data headers: RefCell<Vec<(String, String)>>;

    def __call__(&self, status: String, headers: Vec<(String, String)>) -> PyResult<PyObject> {
        *self.status(py).borrow_mut() = status;
        *self.headers(py).borrow_mut() = headers;
        Ok(py.None())
    }
});
