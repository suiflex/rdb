//! Demo seeding for design parity: `RDBS_MOCK=1` swaps the user's real
//! connection store for an in-memory temp store matching the reference
//! design, and (later) routes "connect" to the in-process MockDriver.

use rdbs_connstore::{ConnStore, Engine, SavedConnection};

fn conn(
    name: &str,
    engine: Engine,
    host: &str,
    port: u16,
    db: Option<&str>,
    group: &str,
    local: bool,
) -> SavedConnection {
    let mut c = SavedConnection::new(name, engine, host, port, "fintech_admin");
    c.database = db.map(str::to_string);
    c.group = Some(group.to_string());
    c.local = local;
    c
}

/// The connection list from design/1-connections.png: OSS 3 · LOCAL 4 ·
/// PROFIN 5 · SPMB 6 · UNGROUPED 9.
pub fn mock_store(dir: std::path::PathBuf) -> ConnStore {
    let _ = std::fs::create_dir_all(&dir);
    // File backend only: demo mode must never touch the OS keychain.
    let backend =
        Box::new(rdbs_connstore::EncryptedFileBackend::new(&dir).expect("file secret backend"));
    let mut store = ConnStore::new(dir.join("connections.json"), backend);

    let host = "128.199.74.52";
    let mut add = |c: SavedConnection| {
        let _ = store.add(c);
    };

    for (name, db) in [
        ("oss rba", "oss_rba_master"),
        ("jdih bkpm", "jdih_bkpm_2025"),
        ("primbon", "primbon"),
    ] {
        add(conn(
            name,
            Engine::Postgres,
            host,
            5432,
            Some(db),
            "OSS",
            false,
        ));
    }
    for (name, engine, port, db) in [
        ("pg local", Engine::Postgres, 5432, Some("postgres")),
        ("mysql local", Engine::MySql, 3306, Some("mysql")),
        ("redis local", Engine::Redis, 6379, None),
        ("mongo local", Engine::Mongo, 27017, Some("local")),
    ] {
        add(conn(name, engine, "127.0.0.1", port, db, "LOCAL", true));
    }
    // PROFIN — the expanded group in the reference.
    add(conn(
        "portfolio",
        Engine::Postgres,
        host,
        5432,
        Some("portfolio"),
        "PROFIN",
        false,
    ));
    {
        let mut c = conn(
            "bot ai tele",
            Engine::Postgres,
            host,
            5432,
            Some("ai_bot_fintech"),
            "PROFIN",
            true,
        );
        c.sslmode = rdbs_core::conn::SslMode::Require;
        c.tags = vec!["profin".into(), "fintech".into()];
        add(c);
    }
    add(conn(
        "profin",
        Engine::Postgres,
        host,
        5432,
        Some("profin"),
        "PROFIN",
        false,
    ));
    add(conn(
        "POS",
        Engine::Postgres,
        host,
        5432,
        Some("pos"),
        "PROFIN",
        false,
    ));
    add(conn(
        "redis portfolio",
        Engine::Redis,
        host,
        6379,
        None,
        "PROFIN",
        false,
    ));

    for (name, db) in [
        ("spmb pusat", "spmb"),
        ("spmb jabar", "spmb_jabar"),
        ("spmb jatim", "spmb_jatim"),
        ("spmb banten", "spmb_banten"),
        ("spmb dki", "spmb_dki"),
        ("spmb diy", "spmb_diy"),
    ] {
        add(conn(
            name,
            Engine::Postgres,
            host,
            5432,
            Some(db),
            "SPMB",
            false,
        ));
    }
    for (name, engine, port, db) in [
        ("suitest", Engine::Postgres, 5432, Some("suitest")),
        ("suitest test", Engine::Postgres, 5432, Some("suitest_test")),
        ("rtmanagement", Engine::Postgres, 5432, Some("rtmanagement")),
        ("analytics", Engine::MySql, 3306, Some("analytics")),
        ("billing", Engine::MySql, 3306, Some("billing")),
        ("cache edge", Engine::Redis, 6379, None),
        ("queue", Engine::Redis, 6379, None),
        ("iot events", Engine::Mongo, 27017, Some("iot")),
        ("logs", Engine::Mongo, 27017, Some("logs")),
    ] {
        add(conn(name, engine, host, port, db, "", false));
    }
    store
}

/// True when the app runs in design-mock mode.
pub fn mock_mode() -> bool {
    std::env::var("RDBS_MOCK").is_ok_and(|v| v == "1")
}

// ===========================================================================
// MockDriver: an in-process Postgres look-alike with the reference dataset.
// ===========================================================================

use std::sync::Mutex;

use async_trait::async_trait;
use rdbs_core::conn::ConnConfig;
use rdbs_core::driver::Driver;
use rdbs_core::error::{RdbsError, Result};
use rdbs_core::query::Query;
use rdbs_core::result::{Cell, Column, ResultSet, Row};
use rdbs_core::schema::{Container, ContainerKind, Database, Field, Function, Schema};
use rdbs_core::write::{TableRef, WriteOp};

/// (name, weight) — weights sum to 986 like the reference `emiten` table.
const SECTORS: &[(&str, i64)] = &[
    ("Financials", 214),
    ("Consumer Cyclicals", 162),
    ("Basic Materials", 141),
    ("Industrials", 118),
    ("Energy", 97),
    ("Infrastructures", 86),
    ("Consumer Non-Cyclicals", 74),
    ("Properties & Real Estate", 41),
    ("Healthcare", 33),
    ("Technology", 20),
];

/// First 26 rows exactly as visible in the reference workspace screen:
/// (uuid first 24 hex as grouped prefix, code, name, short, id_sector prefix,
/// created_at).
#[rustfmt::skip]
const EMITEN_FIXED: &[(&str, &str, &str, &str, &str, &str)] = &[
    ("07b2b349-5597-d135-edf7-2a9cf662", "93344",    "Bayan Resources",              "BYAN", "d1889f8a-8ee1-3e6b-8755-5ade3978", "2025-09-27 15:06:31.138"),
    ("2015c876-fb75-b494-1d61-f0c919e3", "28806303", "Adaro Minerals Indonesia Tbk", "ADMR", "60606760-fc31-3bcf-de3b-8d30dee3", "2025-09-27 15:07:14.671"),
    ("26f7bdc4-d540-bce7-49a4-ecaa8e6f", "93722",    "Industri Dan Perdagangan",     "CARS", "d31d69d6-5804-ff98-31e4-4fa3433e", "2025-09-27 15:06:38.514"),
    ("32bb4ef6-95af-eb5e-8e32-5f8f0a9f", "15313498", "Diagnos Laboratorium Utama",   "DGNS", "48634fa5-8572-9ea8-e9d7-a79aa620", "2025-09-27 15:06:41.353"),
    ("3fe67583-4b77-f81e-8a12-8f533765", "93854",    "Optima Prima",                 "OPMS", "b20436ee-a6a3-e3bd-895a-94735727", "2025-09-27 15:06:48.138"),
    ("41ecc844-9b2f-e05d-830c-fc24534f", "93824",    "PT Wahana Interfood",          "COCO", "33f45b3e-8b2e-82ca-111f-b5b251d5", "2025-09-27 15:06:40.642"),
    ("503f9f22-8466-6a65-e579-893545a3", "93498",    "Mitra Investindo",             "MITI", "509376be-c72d-8de7-29c3-68f7ddb0", "2025-09-27 15:08:58.834"),
    ("6658fc70-21c8-0cbb-7abe-2c68fc93", "93248",    "Maskapai Reasuransi",          "MREI", "9f42b812-82ff-8477-4441-f2712e68", "2025-09-27 15:07:31.144"),
    ("6da71922-d386-c97d-963b-eecca221", "93715",    "Prodia Widyahusada",           "PRDA", "49d56feb-08e3-1064-dd90-e5957cbd", "2025-09-27 15:06:48.598"),
    ("725c1a01-e0c3-4488-faa6-971af20e", "93423",    "DFI Retail Nusantara Tbk Pt",  "HERO", "a7c1d62f-faba-604c-3aba-2e8dc3d9", "2025-09-27 15:06:37.388"),
    ("7e409e8a-dab7-64d1-6b0c-a67165ef", "93645",    "Golden Plantation",            "GOLL", "ba0f2023-fd40-0fbe-f6df-a2c18848", "2025-09-27 15:08:42.713"),
    ("7f228a3c-32a9-83e8-b0f9-405bc4d7", "60275922", "Atlantis Subsea Indonesia",    "ATLA", "d12f3789-7a2f-d538-c774-70d56480", "2025-09-27 15:07:49.148"),
    ("8717ea67-9b80-2116-dda1-04b4daee", "93306",    "Asia Pacific Fibers",          "POLY", "744b8791-cbe1-0053-3f9d-5a0bc63c", "2025-09-27 15:07:52.812"),
    ("95777a3a-c925-2d18-c407-1c73988d", "41289108", "Black Diamond Resources Tbk",  "COAL", "c8143c72-49bb-f7de-bd53-0894ffbf", "2025-09-27 15:07:41.999"),
    ("95af6cb0-ae49-e76d-a578-f291ac69", "47588533", "Mitra Pack",                   "PTMP", "f2ab98ae-c001-e008-0ba0-6d53ba0a", "2025-09-27 15:07:30.631"),
    ("97da2849-d73e-b438-a907-d090402c", "93317",    "Bank Artha Graha",             "INPC", "7c570089-3a34-bd83-441a-abe1f6f2", "2025-09-27 15:07:54.169"),
    ("98787790-b0b2-5be4-9e70-9777c1b9", "12595224", "Bhakti Multi Artha",           "BHAT", "e087a89d-afff-cfc2-1f07-dac5d7ea", "2025-09-27 15:06:16.315"),
    ("9eab4200-c101-4771-bf77-61f91210", "93527",    "Paninvest",                    "PNIN", "36c127f6-11f9-f766-1ca2-ef964b32", "2025-09-27 15:06:54.425"),
    ("a148123d-22b7-509a-b29c-2c065951", "93392",    "Elnusa",                       "ELSA", "f9c8ea2e-e06f-7874-85e4-4c83b5a9", "2025-09-27 15:06:46.816"),
    ("ac8c8211-04aa-8f06-54d8-3ff48dea", "93532",    "Pelat Timah Nusantara",        "NIKL", "c8a88ffc-4aad-6e5e-658d-9db24f74", "2025-09-27 15:06:51.912"),
    ("add957ea-5fa5-d250-1abf-e7b24204", "93826",    "Capri Nusa Satu Properti",     "CPRI", "c3a87776-5428-488a-6202-f1d681d0", "2025-09-27 15:06:57.775"),
    ("bb057b1e-9c98-f30b-1fd8-0ed31f62", "60275923", "Multi Hanna Kreasindo Tbk",    "MHKI", "ee0e54ce-8e2d-fb3c-e884-1df0b98a", "2025-09-27 15:06:15.176"),
    ("bc0d3851-4ef8-30c0-e5ff-c2541a89", "121965",   "Diamond Citra",                "DADA", "a96c361f-3399-8665-6b19-aaed7c27", "2025-09-27 15:08:31.931"),
    ("be4b28d1-f9e6-9595-b47d-bfe6841b", "21561164", "Kedoya Adyaraya Tbk PT",       "RSGK", "7d2f2bb3-62a5-3ae5-c62d-50baee39", "2025-09-27 15:08:24.347"),
    ("cbcfcd9e-5805-3b3a-3c40-23ff55eb", "93615",    "Wahana Ottomitra",             "WOMF", "ebcdbb34-63f2-8d5e-fb23-8e6d1096", "2025-09-27 15:06:11.274"),
    ("e17d47c5-b5f0-fb74-4543-0b240c1b", "93309",    "Astra Graphia",                "ASGR", "94cdba75-9f46-64a5-3ac8-fde986d4", "2025-09-27 15:08:18.314"),
];

/// Tiny deterministic RNG (xorshift64*) so mock data is stable across runs.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn hex(rng: &mut Rng, n: usize) -> String {
    (0..n)
        .map(|_| char::from_digit((rng.below(16)) as u32, 16).unwrap())
        .collect()
}

fn uuid_from(rng: &mut Rng) -> String {
    format!(
        "{}-{}-{}-{}-{}",
        hex(rng, 8),
        hex(rng, 4),
        hex(rng, 4),
        hex(rng, 4),
        hex(rng, 12)
    )
}

fn emiten_columns() -> Vec<Column> {
    let col = |name: &str, ty: &str| Column {
        name: name.into(),
        type_name: ty.into(),
    };
    vec![
        col("id", "uuid"),
        col("code", "varchar"),
        col("name", "varchar"),
        col("short_name", "varchar"),
        col("country", "varchar"),
        col("ccy", "varchar"),
        col("exch", "varchar"),
        col("id_sector", "fk"),
        col("created_at", "timestamptz"),
        col("updated_at", "timestamptz"),
    ]
}

fn gen_emiten() -> Vec<Row> {
    let mut rng = Rng(0x5EED_CAFE_F00D_0001);
    let sector_ids: Vec<String> = {
        let mut r = Rng(0xA11C_E5EC_7012_3456);
        SECTORS.iter().map(|_| uuid_from(&mut r)).collect()
    };
    let mut rows: Vec<Row> = Vec::with_capacity(986);
    let text = |s: &str| Cell::Text(s.to_string());
    for (id, code, name, short, sector, ts) in EMITEN_FIXED {
        let mut r = Rng(id.as_bytes().iter().map(|&b| b as u64).sum::<u64>() + 7);
        rows.push(vec![
            Cell::Text(format!("{id}-{}", hex(&mut r, 12))),
            text(code),
            text(name),
            text(short),
            text("indonesia"),
            text("IDR"),
            text("Jakarta"),
            Cell::Text(format!("{sector}-{}", hex(&mut r, 12))),
            text(ts),
            Cell::Null,
        ]);
    }
    const A: &[&str] = &[
        "Astra", "Bumi", "Citra", "Duta", "Elang", "Fajar", "Graha", "Harum", "Indah", "Jaya",
        "Karya", "Lestari", "Mega", "Nusa", "Optima", "Prima", "Quanta", "Raya", "Sentosa",
        "Tirta", "Utama", "Wahana",
    ];
    const B: &[&str] = &[
        "Abadi",
        "Buana",
        "Cemerlang",
        "Dinamika",
        "Energi",
        "Finansial",
        "Gemilang",
        "Harmoni",
        "Investama",
        "Kapital",
        "Logistik",
        "Mandiri",
        "Niaga",
        "Pratama",
        "Resources",
        "Sejahtera",
        "Teknologi",
        "Ventura",
    ];
    for i in 0..(986 - EMITEN_FIXED.len()) {
        let a = A[rng.below(A.len() as u64) as usize];
        let b = B[rng.below(B.len() as u64) as usize];
        let name = format!("{a} {b}");
        let short: String = format!(
            "{}{}{}{}",
            &a[0..1],
            a[1..2].to_uppercase(),
            &b[0..1],
            b[1..2].to_uppercase()
        )
        .to_uppercase();
        // Weighted sector pick keeps the per-sector totals plausible.
        let mut pick = rng.below(986) as i64;
        let mut sector = 0;
        for (idx, (_, w)) in SECTORS.iter().enumerate() {
            if pick < *w {
                sector = idx;
                break;
            }
            pick -= w;
        }
        let ts = format!(
            "2025-09-27 15:{:02}:{:02}.{:03}",
            6 + rng.below(3),
            rng.below(60),
            rng.below(1000)
        );
        let id = format!("f{}", &uuid_from(&mut rng)[1..]);
        rows.push(vec![
            Cell::Text(id),
            Cell::Text(format!("{}", 93000 + i as i64 * 7 % 7000)),
            Cell::Text(name),
            Cell::Text(short),
            Cell::Text("indonesia".into()),
            Cell::Text("IDR".into()),
            Cell::Text("Jakarta".into()),
            Cell::Text(sector_ids[sector].clone()),
            Cell::Text(ts),
            Cell::Null,
        ]);
    }
    // The reference grid is sorted by id ascending.
    rows.sort_by(|a, b| match (&a[0], &b[0]) {
        (Cell::Text(x), Cell::Text(y)) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    });
    rows
}

struct MockTable {
    cols: Vec<Column>,
    rows: Vec<Row>,
}

fn simple_table(names: &[(&str, &str)], rows: Vec<Row>) -> MockTable {
    MockTable {
        cols: names
            .iter()
            .map(|(n, t)| Column {
                name: (*n).into(),
                type_name: (*t).into(),
            })
            .collect(),
        rows,
    }
}

fn uuid_fn(name: &str, args: &str, body: &str) -> Function {
    Function {
        name: name.into(),
        definition: format!(
            "CREATE OR REPLACE FUNCTION public.{name}({args})\nRETURNS uuid\nLANGUAGE c\nIMMUTABLE PARALLEL SAFE STRICT\nAS '$libdir/uuid-ossp', $function${body}$function$\n"
        ),
    }
}

pub struct MockDriver {
    tables: Mutex<std::collections::BTreeMap<String, MockTable>>,
}

impl MockDriver {
    fn build() -> Self {
        let mut r = Rng(0xA11C_E5EC_7012_3456);
        let mut tables = std::collections::BTreeMap::new();
        tables.insert(
            "emiten".to_string(),
            MockTable {
                cols: emiten_columns(),
                rows: gen_emiten(),
            },
        );
        let sector_rows: Vec<Row> = SECTORS
            .iter()
            .map(|(n, _)| {
                vec![
                    Cell::Text(uuid_from(&mut r)),
                    Cell::Text((*n).to_string()),
                    Cell::Text("2025-09-27 14:58:03.117".into()),
                ]
            })
            .collect();
        tables.insert(
            "sectors".to_string(),
            simple_table(
                &[
                    ("id", "uuid"),
                    ("name", "text"),
                    ("created_at", "timestamptz"),
                ],
                sector_rows,
            ),
        );
        let referral_rows: Vec<Row> = [
            "organic",
            "ads",
            "referral",
            "partner",
            "affiliate",
            "sosmed",
            "other",
        ]
        .iter()
        .enumerate()
        .map(|(i, n)| {
            vec![
                Cell::Int(i as i64 + 1),
                Cell::Text((*n).to_string()),
                Cell::Null,
            ]
        })
        .collect();
        tables.insert(
            "referral_sources".to_string(),
            simple_table(
                &[("id", "int4"), ("name", "text"), ("notes", "text")],
                referral_rows,
            ),
        );
        let mut rng = Rng(0xBEEF_BEEF_0000_0001);
        let tx_rows: Vec<Row> = (0..500)
            .map(|i| {
                vec![
                    Cell::Int(i + 1),
                    Cell::Text(uuid_from(&mut rng)),
                    Cell::Float((rng.below(900_000) as f64 + 1000.0) / 100.0),
                    Cell::Text("IDR".into()),
                    Cell::Text(format!(
                        "2025-09-{:02} 09:{:02}:{:02}",
                        1 + rng.below(27),
                        rng.below(60),
                        rng.below(60)
                    )),
                ]
            })
            .collect();
        tables.insert(
            "transactions".to_string(),
            simple_table(
                &[
                    ("id", "int8"),
                    ("emiten_id", "fk"),
                    ("amount", "numeric"),
                    ("ccy", "varchar"),
                    ("created_at", "timestamptz"),
                ],
                tx_rows,
            ),
        );
        let user_rows: Vec<Row> = (0..128)
            .map(|i| {
                vec![
                    Cell::Int(i + 1),
                    Cell::Text(format!("user{:03}@fintech.id", i + 1)),
                    Cell::Text(if i % 3 == 0 { "admin" } else { "member" }.into()),
                    Cell::Null,
                ]
            })
            .collect();
        tables.insert(
            "users".to_string(),
            simple_table(
                &[
                    ("id", "int8"),
                    ("email", "varchar"),
                    ("role", "varchar"),
                    ("deleted_at", "timestamptz"),
                ],
                user_rows,
            ),
        );
        MockDriver {
            tables: Mutex::new(tables),
        }
    }
}

/// Case-insensitive `LIMIT n` / `OFFSET n` extraction.
fn keyword_num(sql_upper: &str, sql: &str, kw: &str) -> Option<u64> {
    let at = sql_upper.find(kw)?;
    sql[at + kw.len()..]
        .split_whitespace()
        .next()?
        .trim_end_matches(';')
        .parse()
        .ok()
}

/// Table name after FROM, unquoting `"schema"."name"` / backticks.
fn from_table(sql_upper: &str, sql: &str) -> Option<String> {
    let at = sql_upper.find(" FROM ")?;
    let tok = sql[at + 6..].split_whitespace().next()?;
    let tok = tok.trim_end_matches(';');
    let name = tok.rsplit('.').next().unwrap_or(tok);
    Some(name.trim_matches(['"', '`']).to_string())
}

#[async_trait]
impl Driver for MockDriver {
    async fn connect(_cfg: &ConnConfig) -> Result<Self> {
        // A touch of latency so timing readouts look believable.
        tokio::time::sleep(std::time::Duration::from_millis(8)).await;
        Ok(MockDriver::build())
    }

    async fn ping(&self) -> Result<()> {
        Ok(())
    }

    async fn schema(&self) -> Result<Schema> {
        let tables = self.tables.lock().unwrap();
        let containers = tables
            .iter()
            .map(|(name, t)| Container {
                name: name.clone(),
                kind: ContainerKind::Table,
                fields: t
                    .cols
                    .iter()
                    .map(|c| Field {
                        name: c.name.clone(),
                        type_name: c.type_name.clone(),
                        nullable: matches!(c.name.as_str(), "updated_at" | "deleted_at" | "notes"),
                    })
                    .collect(),
            })
            .collect();
        let functions = vec![
            uuid_fn("uuid_generate_v1", "", "uuid_generate_v1"),
            uuid_fn("uuid_generate_v1mc", "", "uuid_generate_v1mc"),
            uuid_fn(
                "uuid_generate_v3",
                "namespace uuid, name text",
                "uuid_generate_v3",
            ),
            uuid_fn("uuid_generate_v4", "", "uuid_generate_v4"),
            uuid_fn(
                "uuid_generate_v5",
                "namespace uuid, name text",
                "uuid_generate_v5",
            ),
            uuid_fn("uuid_nil", "", "uuid_nil"),
            uuid_fn("uuid_ns_dns", "", "uuid_ns_dns"),
            uuid_fn("uuid_ns_oid", "", "uuid_ns_oid"),
            uuid_fn("uuid_ns_url", "", "uuid_ns_url"),
            uuid_fn("uuid_ns_x500", "", "uuid_ns_x500"),
        ];
        Ok(Schema {
            databases: vec![Database {
                name: "ai_bot_fintech".into(),
                containers,
                functions,
            }],
        })
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        // Believable latency for the “● 12 ms” readout.
        tokio::time::sleep(std::time::Duration::from_millis(11)).await;
        let Query::Sql(sql) = q else {
            return Err(RdbsError::UnsupportedQuery);
        };
        let sql = sql.trim();
        let upper = sql.to_uppercase();

        // Aggregate demo: the saved "emiten-per-sektor" query.
        if upper.contains("GROUP BY") && upper.contains("SECTORS") {
            let max = SECTORS[0].1 as f64;
            let rows: Vec<Row> = SECTORS
                .iter()
                .map(|(n, c)| {
                    vec![
                        Cell::Text((*n).to_string()),
                        Cell::Int(*c),
                        Cell::Float(*c as f64 / max),
                    ]
                })
                .collect();
            return Ok(ResultSet::Tabular {
                cols: vec![
                    Column {
                        name: "sector".into(),
                        type_name: "text".into(),
                    },
                    Column {
                        name: "total".into(),
                        type_name: "int8".into(),
                    },
                    Column {
                        name: "share".into(),
                        type_name: "bar".into(),
                    },
                ],
                rows,
            });
        }

        if upper.starts_with("SELECT") {
            let Some(table) = from_table(&upper, sql) else {
                // A `FROM`-less SELECT (e.g. `SELECT 1`, `SELECT now()`): echo
                // the projection as a single scalar row so smoke tests and
                // multi-statement scripts behave like a real engine.
                if !upper.contains(" FROM ") {
                    let expr = sql[6..].trim().trim_end_matches(';').trim();
                    return Ok(ResultSet::Tabular {
                        cols: vec![Column {
                            name: "?column?".into(),
                            type_name: "text".into(),
                        }],
                        rows: vec![vec![Cell::Text(expr.to_string())]],
                    });
                }
                return Err(RdbsError::UnsupportedQuery);
            };
            let tables = self.tables.lock().unwrap();
            let Some(t) = tables.get(&table) else {
                return Err(RdbsError::Query(format!(
                    "relation \"{table}\" does not exist"
                )));
            };
            let limit = keyword_num(&upper, sql, "LIMIT ").unwrap_or(u64::MAX) as usize;
            let offset = keyword_num(&upper, sql, "OFFSET ").unwrap_or(0) as usize;
            let rows: Vec<Row> = t.rows.iter().skip(offset).take(limit).cloned().collect();
            return Ok(ResultSet::Tabular {
                cols: t.cols.clone(),
                rows,
            });
        }

        if upper.starts_with("INSERT") || upper.starts_with("UPDATE") || upper.starts_with("DELETE")
        {
            return Ok(ResultSet::Affected(1));
        }
        Ok(ResultSet::Affected(0))
    }

    async fn primary_key(&self, table: &TableRef) -> Result<Vec<String>> {
        Ok(match table.name.as_str() {
            "emiten" | "sectors" => vec!["id".into()],
            "referral_sources" | "transactions" | "users" => vec!["id".into()],
            _ => Vec::new(),
        })
    }

    async fn count(&self, table: &TableRef) -> Result<u64> {
        let tables = self.tables.lock().unwrap();
        tables
            .get(&table.name)
            .map(|t| t.rows.len() as u64)
            .ok_or_else(|| RdbsError::Query(format!("relation \"{}\" does not exist", table.name)))
    }

    async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        let mut tables = self.tables.lock().unwrap();
        let mut applied = 0u64;
        for op in ops {
            let t = tables
                .get_mut(&op.table().name)
                .ok_or_else(|| RdbsError::Query("unknown table".into()))?;
            let find = |rows: &Vec<Row>, pk: &[(String, Cell)]| -> Option<usize> {
                let pk_idx: Vec<(usize, &Cell)> = pk
                    .iter()
                    .filter_map(|(col, v)| {
                        t.cols.iter().position(|c| &c.name == col).map(|i| (i, v))
                    })
                    .collect();
                rows.iter().position(|r| {
                    pk_idx
                        .iter()
                        .all(|(i, v)| r.get(*i).map(Cell::render) == Some(v.render()))
                })
            };
            match op {
                WriteOp::Update { pk, changes, .. } => {
                    if let Some(ri) = find(&t.rows, pk) {
                        for (col, v) in changes {
                            if let Some(ci) = t.cols.iter().position(|c| &c.name == col) {
                                t.rows[ri][ci] = v.clone();
                            }
                        }
                        applied += 1;
                    }
                }
                WriteOp::Insert { values, .. } => {
                    let mut row: Row = vec![Cell::Null; t.cols.len()];
                    for (col, v) in values {
                        if let Some(ci) = t.cols.iter().position(|c| &c.name == col) {
                            row[ci] = v.clone();
                        }
                    }
                    t.rows.push(row);
                    applied += 1;
                }
                WriteOp::Delete { pk, .. } => {
                    if let Some(ri) = find(&t.rows, pk) {
                        t.rows.remove(ri);
                        applied += 1;
                    }
                }
            }
        }
        Ok(applied)
    }

    async fn close(self) -> Result<()> {
        Ok(())
    }
}
