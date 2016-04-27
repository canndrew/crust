use std::fs::File;
use std::ffi::OsString;
use std::path::PathBuf;
use std::io::Write;

use config_file_handler;
use config_file_handler::FileHandler;
use transport::ListenEndpoint;
use socket_addr::SocketAddr;
use static_contact_info::StaticContactInfo;

use error::Error;

/// Used to configure a `Service` via the `Service::with_config` method.
#[derive(PartialEq, Eq, Debug, RustcDecodable, RustcEncodable, Clone)]
pub struct Config {
    /// Peers to connect to during bootstrap.
    pub hard_coded_contacts: Vec<StaticContactInfo>,
    /// Endpoints to listen on.
    pub listen_endpoints: Vec<ListenEndpoint>,
    /// Known udp mapper servers.
    pub udp_mapper_servers: Vec<SocketAddr>,
    /// Known tcp mapper servers.
    pub tcp_mapper_servers: Vec<SocketAddr>,
    /// Port for discovering other peers on the local network.
    pub service_discovery_port: Option<u16>,
    /// Name of the bootrap cache file.
    pub bootstrap_cache_name: Option<String>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            hard_coded_contacts: vec![],
            listen_endpoints: vec![unwrap_result!("tcp-listen://0.0.0.0:0".parse())],
            udp_mapper_servers: vec![],
            tcp_mapper_servers: vec![],
            service_discovery_port: None,
            bootstrap_cache_name: None,
        }
    }
}

impl Config {
    /// Get the name of the crust config file.
    pub fn get_file_name() -> Result<OsString, Error> {
        let mut name = try!(config_file_handler::exe_file_stem());
        name.push(".crust.config");
        Ok(name)
    }

    /// Reads the default crust config file.
    pub fn read() -> Result<Config, Error> {
        let file_handler = try!(FileHandler::new(&try!(Config::get_file_name())));
        let cfg = try!(file_handler.read_file());
        Ok(cfg)
    }

    /// Writes a Crust config file **for use by tests and examples**.
    ///
    /// The file is written to the [`current_bin_dir()`](file_handler/fn.current_bin_dir.html)
    /// with the appropriate file name.
    ///
    /// N.B. This method should only be used as a utility for test and examples.  In normal use
    /// cases, this file should be created by the installer for the dependent application.
    pub fn write(&self) -> Result<PathBuf, Error> {
        let mut config_path = try!(config_file_handler::current_bin_dir());
        config_path.push(try!(Self::get_file_name()));
        let mut file = try!(File::create(&config_path));
        try!(write!(&mut file,
                    "{}",
                    ::rustc_serialize::json::as_pretty_json(self)));
        try!(file.sync_all());
        Ok(config_path)
    }
}

