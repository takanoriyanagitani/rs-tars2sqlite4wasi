use std::fs::File;
use std::io;

use io::BufRead;
use io::BufReader;
use io::Read;

use tar::Archive;
use tar::Entry;
use tar::Header;

use flate2::bufread::GzDecoder;

use rusqlite::Connection;
use rusqlite::Statement;
use rusqlite::Transaction;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryType {
    Reg = 0,
    Link = 1,
    Symlink = 2,
    Char = 3,
    Block = 4,
    Dir = 5,
    Fifo = 6,
    Continuous = 7,
    GNULongName = b'L',
    GNULongLink = b'K',
    GNUSparse = b'S',
    XGlobalHeader = b'g',
    XHeader = b'x',
}

impl TryFrom<tar::EntryType> for EntryType {
    type Error = io::Error;

    fn try_from(e: tar::EntryType) -> Result<Self, Self::Error> {
        match e {
            tar::EntryType::Regular => Ok(Self::Reg),
            tar::EntryType::Link => Ok(Self::Link),
            tar::EntryType::Symlink => Ok(Self::Symlink),
            tar::EntryType::Char => Ok(Self::Char),
            tar::EntryType::Block => Ok(Self::Block),
            tar::EntryType::Directory => Ok(Self::Dir),
            tar::EntryType::Fifo => Ok(Self::Fifo),
            tar::EntryType::Continuous => Ok(Self::Continuous),
            tar::EntryType::GNULongName => Ok(Self::GNULongName),
            tar::EntryType::GNULongLink => Ok(Self::GNULongLink),
            tar::EntryType::GNUSparse => Ok(Self::GNUSparse),
            tar::EntryType::XGlobalHeader => Ok(Self::XGlobalHeader),
            tar::EntryType::XHeader => Ok(Self::XHeader),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported entry type",
            )),
        }
    }
}

pub const CREATE_TAR_SQL: &str = r#"
    CREATE TABLE IF NOT EXISTS tars (
        archive_id INTEGER PRIMARY KEY,
        path TEXT NOT NULL UNIQUE
    )
"#;

pub const CREATE_ENT_SQL: &str = r#"
    CREATE TABLE IF NOT EXISTS entries (
        archive_id INTEGER NOT NULL,
        entry_path TEXT NOT NULL,
        size INTEGER NOT NULL,
        mtime INTEGER NOT NULL,
        entry_type INTEGER,
        content BLOB NOT NULL,
        PRIMARY KEY (archive_id, entry_path),
        FOREIGN KEY (archive_id) REFERENCES tars(archive_id)
    )
    WITHOUT ROWID
"#;

pub const INSERT_TAR_SQL: &str = r#"
    INSERT INTO tars (path) VALUES (?)
    ON CONFLICT (path) DO NOTHING
    RETURNING rowid
"#;

pub const UPSERT_ENT_SQL: &str = r#"
    INSERT INTO entries (
        archive_id,
        entry_path,
        size,
        mtime,
        entry_type,
        content
    )
    VALUES (?, ?, ?, ?, ?, ?)
    ON CONFLICT(archive_id, entry_path) DO UPDATE SET
        size = excluded.size,
        mtime = excluded.mtime,
        entry_type = excluded.entry_type,
        content = excluded.content
"#;

pub fn entry2sqlite<R>(
    archive_id: u32,
    e: &mut Entry<R>,
    max_size: u64,
    buf: &mut Vec<u8>,
    stmt: &mut Statement,
) -> Result<usize, io::Error>
where
    R: Read,
{
    buf.clear();
    let mut taken = e.take(max_size);
    taken.read_to_end(buf)?;

    let epat = e.path().map_err(io::Error::other)?;
    let ospat = epat.as_os_str();
    let lossy = ospat.to_string_lossy();
    let spat: &str = &lossy;
    let entry_path: &str = spat;
    let size: i64 = e.size().try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "unable to convert the size to i64",
        )
    })?;

    let hdr: &Header = e.header();
    let mtime: i64 = hdr.mtime().unwrap_or_default().try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "unable to convert mtime to i64",
        )
    })?;
    let tent = hdr.entry_type();
    let oent: Option<EntryType> = tent.try_into().ok();
    let oraw: Option<u8> = oent.map(|e| e as u8);

    let content: &[u8] = buf;

    stmt.execute((archive_id, entry_path, size, mtime, oraw, content))
        .map_err(io::Error::other)
}

pub fn tar2sqlite<R>(
    archive_path: &str,
    mut archive: Archive<R>,
    max_size: u64,
    buf: &mut Vec<u8>,
    conn: &mut Connection,
) -> Result<usize, io::Error>
where
    R: Read,
{
    let rowid: u32 = conn
        .query_one(INSERT_TAR_SQL, [archive_path], |row| row.get(0))
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("the tar {archive_path} already inserted: {e}"),
            )
        })?;

    let tx: Transaction = conn.transaction().map_err(io::Error::other)?;

    let mut stmt: Statement = tx.prepare(UPSERT_ENT_SQL).map_err(io::Error::other)?;

    let ents = archive.entries().map_err(io::Error::other)?;
    let mut chg_tot: usize = 0;
    for rent in ents {
        let mut ent: Entry<R> = rent?;
        let changed: usize = entry2sqlite(rowid, &mut ent, max_size, buf, &mut stmt)?;
        chg_tot += changed;
    }
    stmt.finalize().map_err(io::Error::other)?;
    tx.commit().map_err(io::Error::other)?;
    Ok(chg_tot)
}

pub fn tarpath2sqlite(
    tar_path: &str,
    gzipped: bool,
    max_size: u64,
    buf_cap: usize,
    buf: &mut Vec<u8>,
    conn: &mut Connection,
) -> Result<usize, io::Error> {
    let f: File = File::open(tar_path)?;
    let brdr = BufReader::with_capacity(buf_cap, f);
    match gzipped {
        false => {
            let a: Archive<_> = Archive::new(brdr);
            tar2sqlite(tar_path, a, max_size, buf, conn)
        }
        true => {
            let gdec: GzDecoder<_> = GzDecoder::new(brdr);
            let a: Archive<_> = Archive::new(gdec);
            tar2sqlite(tar_path, a, max_size, buf, conn)
        }
    }
}

pub fn paths2tars2sqlite<R, C>(
    mut paths: R,
    gzip_detector: C,
    max_size: u64,
    buf_cap: usize,
    buf: &mut Vec<u8>,
    conn: &mut Connection,
) -> Result<usize, io::Error>
where
    R: BufRead,
    C: Fn(&str) -> Result<bool, io::Error>,
{
    conn.execute(CREATE_TAR_SQL, ()).map_err(io::Error::other)?;
    conn.execute(CREATE_ENT_SQL, ()).map_err(io::Error::other)?;

    let mut path_buf: String = String::default();
    let mut tot: usize = 0;
    loop {
        path_buf.clear();

        let pread: usize = paths.read_line(&mut path_buf)?;
        if 0 == pread {
            return Ok(tot);
        }

        let nolf: &str = path_buf.trim_end();
        let is_gzip: bool = gzip_detector(nolf)?;
        let changed: usize = tarpath2sqlite(nolf, is_gzip, max_size, buf_cap, buf, conn)?;
        tot += changed;
    }
}

pub fn gzip_detector_simple(path: &str) -> Result<bool, io::Error> {
    Ok(path.ends_with(".gz"))
}

pub fn stdin2paths2tars2sqlite<C>(
    gzip_detector: C,
    max_size: u64,
    buf_cap: usize,
    buf: &mut Vec<u8>,
    conn: &mut Connection,
) -> Result<usize, io::Error>
where
    C: Fn(&str) -> Result<bool, io::Error>,
{
    paths2tars2sqlite(
        io::stdin().lock(),
        gzip_detector,
        max_size,
        buf_cap,
        buf,
        conn,
    )
}

pub fn stdin2paths2tars2sqlite_default(
    max_size: u64,
    buf_cap: usize,
    buf: &mut Vec<u8>,
    conn: &mut Connection,
) -> Result<usize, io::Error> {
    stdin2paths2tars2sqlite(gzip_detector_simple, max_size, buf_cap, buf, conn)
}

pub const BUFFER_CAP_FOR_BUF_RDR_DEFAULT: usize = 1048576;
pub const BUFFER_SIZE_DEFAULT: usize = 8192;

pub struct Config {
    pub max_entry_size: u64,
    pub buffer_capacity4bufrdr: usize,

    /// The size of the buffer for entries. e.g., equal to max_entry_size.
    pub buffer_size: usize,
    pub sqlite_path: String,
}

impl Config {
    pub fn stdin2paths2tars2sqlite_default(&self) -> Result<usize, io::Error> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.buffer_size);
        let mut con: Connection = Connection::open(&self.sqlite_path).map_err(io::Error::other)?;
        let tot: usize = stdin2paths2tars2sqlite_default(
            self.max_entry_size,
            self.buffer_capacity4bufrdr,
            &mut buf,
            &mut con,
        )?;
        con.close()
            .map(|_| tot)
            .map_err(|p| p.1)
            .map_err(io::Error::other)
    }
}
