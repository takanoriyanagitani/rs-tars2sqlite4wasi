use std::io;
use std::process::ExitCode;

use rs_tars2sqlite4wasi::BUFFER_CAP_FOR_BUF_RDR_DEFAULT;
use rs_tars2sqlite4wasi::BUFFER_SIZE_DEFAULT;
use rs_tars2sqlite4wasi::Config;

fn io_envkey2val(key: &'static str) -> impl Fn() -> Result<String, io::Error> {
    move || std::env::var(key).map_err(io::Error::other)
}

fn string2usize(s: String) -> Result<usize, io::Error> {
    str::parse(&s).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid usize {s}: {e}"),
        )
    })
}

fn string2u64(s: String) -> Result<u64, io::Error> {
    str::parse(&s)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("invalid u64 {s}: {e}")))
}

fn bind<I, M, T, U, E>(mut iofn: I, pure: M) -> impl FnMut() -> Result<U, E>
where
    I: FnMut() -> Result<T, E>,
    M: Fn(T) -> Result<U, E>,
{
    move || {
        let t: T = iofn()?;
        pure(t)
    }
}

fn io_max_entry_size() -> impl FnMut() -> Result<u64, io::Error> {
    bind(io_envkey2val("ENV_MAX_ENTRY_SIZE"), string2u64)
}

fn io_buffer_cap4bufrdr() -> impl FnMut() -> Result<usize, io::Error> {
    bind(io_envkey2val("ENV_BUFCAP4BUF_READER"), string2usize)
}

fn io_bufsz() -> impl FnMut() -> Result<usize, io::Error> {
    bind(io_envkey2val("ENV_BUF_SIZE"), string2usize)
}

fn io_path2sqlite() -> impl Fn() -> Result<String, io::Error> {
    io_envkey2val("ENV_SQLITE_DB_FILE_PATH")
}

fn io_config() -> impl FnMut() -> Result<Config, io::Error> {
    move || {
        let max_entry_size: u64 = io_max_entry_size()()?;
        let buffer_capacity4bufrdr: usize =
            io_buffer_cap4bufrdr()().unwrap_or(BUFFER_CAP_FOR_BUF_RDR_DEFAULT);
        let buffer_size: usize = io_bufsz()().unwrap_or(BUFFER_SIZE_DEFAULT);
        let sqlite_path: String = io_path2sqlite()()?;

        Ok(Config {
            max_entry_size,
            buffer_capacity4bufrdr,
            buffer_size,
            sqlite_path,
        })
    }
}

fn io_print_changed() -> impl FnMut(usize) -> Result<(), io::Error> {
    |chg: usize| {
        println!("changed: {chg}");
        Ok(())
    }
}

fn io_stdin2paths2tars2sqlite_default() -> impl FnMut() -> Result<usize, io::Error> {
    || {
        let cfg: Config = io_config()()?;
        cfg.stdin2paths2tars2sqlite_default()
    }
}

fn io_main() -> impl FnOnce() -> Result<(), io::Error> {
    || {
        let chg: usize = io_stdin2paths2tars2sqlite_default()()?;
        io_print_changed()(chg)
    }
}

fn sub() -> Result<(), io::Error> {
    io_main()()
}

fn main() -> ExitCode {
    sub().map(|_| ExitCode::SUCCESS).unwrap_or_else(|e| {
        eprintln!("{e}");

        let max_entry_size: u64 = io_max_entry_size()().unwrap_or_default();
        let buffer_capacity4bufrdr: usize =
            io_buffer_cap4bufrdr()().unwrap_or(BUFFER_CAP_FOR_BUF_RDR_DEFAULT);
        let buffer_size: usize = io_bufsz()().unwrap_or(BUFFER_SIZE_DEFAULT);
        let sqlite_path: String = io_path2sqlite()().unwrap_or_default();

        eprintln!("ENV_MAX_ENTRY_SIZE: {max_entry_size}");
        eprintln!("ENV_BUFCAP4BUF_READER: {buffer_capacity4bufrdr}");
        eprintln!("ENV_BUF_SIZE: {buffer_size}");
        eprintln!("ENV_SQLITE_DB_FILE_PATH: {sqlite_path}");

        ExitCode::FAILURE
    })
}
