use clap::Clap;

use hyper::{
    Body, Request, Response, Server, Method, StatusCode,
    service::{make_service_fn, service_fn},
};

use reqwest::{Client, Url};

use serde::Deserialize;

use std::path::PathBuf;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::collections::HashMap;
use std::fs::read_to_string;
use std::sync::Arc;

#[derive(Clap)]
#[clap(version = "0.1", author = "Ethan T. <ethanyidong@gmail.com>")]
struct CliOpts {
    // TODO: add link to docs
    /// Path to .toml config file (see {} for configuration docs)
    config: PathBuf,
    /// Host IP address to bind to
    #[clap(short, long, default_value = "0.0.0.0")]
    host: String,
    /// Host port number to bind to
    #[clap(short, long, default_value = "8000")]
    port: u16,
}

#[derive(Deserialize)]
struct Config {
    files: Vec<FileServe>
}

#[derive(Deserialize)]
struct FileServe {
    #[serde(flatten)]
    location: FileLocation,
    rename: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FileLocation {
    Local {
        path: String,
    },
    External {
        url: String,
    }
}

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}

async fn server(file_contents: Arc<HashMap<String, String>>, req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let mut response = Response::new(Body::empty());

    match req.method() {
        &Method::GET => {
            if file_contents.contains_key(&req.uri().path()[1..]) {
                *response.body_mut() = Body::from(file_contents[&req.uri().path()[1..]].clone());
            }
            else {
                *response.status_mut() = StatusCode::NOT_FOUND;
            }
        },
        _ => {
            *response.status_mut() = StatusCode::NOT_FOUND;
        },
    };

    Ok(response)
}

#[tokio::main]
async fn main() {
    // Read CLI options from clap
    let opts = CliOpts::parse();

    // Read config data from config file
    let config_str = read_to_string(opts.config).expect("Error reading config");
    let config_data = toml::from_str::<Config>(&config_str).expect("Error deserializing toml");

    // Get file content
    let mut file_contents = HashMap::new();
    let client = Client::new();
    for file in config_data.files {
        let (mut name, contents) = match file.location {
            FileLocation::Local{path} => {
                // Fetch file from local fs
                let path = PathBuf::from(path);
                let file_name = String::from(path.file_name().expect("Error getting file name").to_str().unwrap());
                let contents = read_to_string(&path).expect("Error reading file");
                
                (file_name, contents)
            },
            FileLocation::External{url} => {
                // Fetch file from url
                let url = Url::parse(&url).expect("Invalid URL");
                let file_name = String::from(url.path_segments().expect("Could not split path").next_back().expect("Error getting file name"));
                let resp = client.get(url).send().await.expect("Error GETting url");
                let contents = resp.text().await.expect("Invalid response body");

                (file_name, contents)
            },
        };

        // Rename if specified
        if let Some(rn) = file.rename {
            name = rn;
        }

        // Record file name and contents
        file_contents.insert(name, contents);
    }

    // Arc-ify
    let file_contents_arc = Arc::new(file_contents);

    // Server bind to SocketAddr from opts
    let addr = SocketAddr::from((opts.host.parse::<IpAddr>().expect("Error parsing IP address"), opts.port));

    // Convert to hyper service
    let make_svc = make_service_fn(move |_conn| {
        let file_contents_arc = file_contents_arc.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                server(file_contents_arc.clone(), req)
            }))
        }
    });

    // Run server
    let server = Server::bind(&addr).serve(make_svc).with_graceful_shutdown(shutdown_signal());

    // Wait for errors, print
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}