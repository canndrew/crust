use config_file_handler;

quick_error! {
    /// Crust's universal error type.
    #[derive(Debug)]
    pub enum Error {
        /// File handling errors
        FileHandler(err: config_file_handler::Error) {
            description("File handling error")
            display("File handling error: {}", err)
            cause(err)
            from()
        }
        /// Wrapper for a `::std::io::Error`
        IoError(err: ::std::io::Error) {
            description("IO error")
            display("IO error: {}", err)
            cause(err)
            from()
        }
    }
}

