# wirejack

`wirejack` is a lightweight intercepting proxy like burp / mitmproxy, but mainly focused on automation,
customisation, and performance rather than manual inspection.

The heavy lifting is done by Rust while the handling logic is Python.


## Usage

A TLS CA certificate will be generated on the first run and saved to `~/.local/share/wirejack/`. You'll need
to import this into your browser to avoid certificate warnings.

You'll also need a Python handler script. This one redirects Google requests to GitHub, and provides a static
response for other requests.

```python
from urllib.parse import urlsplit, urlunsplit
from wirejack import Request, Response

class Handler:
    async def handle_http(self, ctx, request):
        print("Intercepted request for ", request.uri)

        uri = urlsplit(request.uri)
        if 'google.com' in uri.netloc:
            request.uri = urlunsplit((uri.scheme, 'github.com', uri.path, uri.query, uri.fragment))
            return await ctx.forward(request)
        elif 'github' in uri.netloc:
            return await ctx.forward(request)
        else:
            response = Response()
            response.body = b'Hello World'
            return response

```

Now you can start a proxy at `http://127.0.0.1:8080` by default:

```sh
wirejack http handler.py

# Only intercept matching destination domains / ports
wirejack http handler.py -f '*.com' -f '*.org'
wirejack http handler.py -f :443
```


## Extending with Rust

If Python isn't fast enough for your needs, it's relatively easy to add Rust modules to do the
performance-critical work.

Importing Rust modules into the HTTP handler is possible by using `wirejack` in your own crate. Define your
[PyO3](https://pyo3.rs/) modules and `append_to_inittab!` before starting the proxy. Modules can then be
imported into Python as usual.

```rust
use pyo3::prelude::*;
use wirejack::{HttpConfig, proxy_http};

fn main() {
    pyo3::append_to_inittab!(addon);
    proxy_http(HttpConfig {
        handler: "handler.py".into(),
        bind: vec!["127.0.0.1:8080".parse().unwrap()],
        proxy: None,
        filter: vec![],
        interactive: false,
        threads: 1,
    });
}

#[pymodule]
pub mod addon {
    use pyo3::prelude::*;
    use std::time::Duration;
    use wirejack::PyHttpRequest;

    #[pyfunction]
    fn do_stuff(request: &PyHttpRequest) -> String {
        println!("Doing stuff: {}", request.inner().uri());
        "stuff".into()
    }

    #[pyfunction]
    fn sleep(py: Python) -> PyResult<Bound<PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(Python::attach(|py| py.None()))
        })
    }
}
```

```python
import addon

class Handler:
    async def handle_http(self, ctx, request):
        print("Stuff:", addon.do_stuff(request))
        print("Sleep")
        await addon.sleep()
        return await ctx.forward(request)
```
