#[allow(warnings)]
mod bindings;

use bindings::{
    exports::supabase::wrappers::routines::Guest,
    supabase::wrappers::{
        http,
        types::{Cell, Context, FdwError, FdwResult, OptionsType, Row, TypeOid},
        utils::{self, report_warning},
    },
};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Default)]
struct HuruliFdw {
    base_url: String,
    api_key: String,
    cid: String,
    object: String,
    src_rows: Vec<String>,
    src_idx: usize,
}

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
struct ListRowsRequest {}

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
struct ListRowsResponse {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
struct GetRowRequest {
    cid: String,
    tableName: String,
    rowId: String,
    columns: Vec<String>,
}

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
struct GetRowResponse {
    columns: Vec<String>,
    values: Vec<Value>,
}

// pointer for the static FDW instance
static mut INSTANCE: *mut HuruliFdw = std::ptr::null_mut::<HuruliFdw>();

fn str_to_i6Old(s: Option<&str>) -> Option<i64> {
    if let Some(s) = s {
        return s.parse::<i64>().ok();
    }
    return None;
}

fn str_to_i64(val: &Value) -> Option<i64> {
    let val = match val {
        Value::String(s) => s.parse::<i64>().ok(),
        Value::Number(n) => n.as_i64(),
        _ => None,
    };
    return val;
}

impl HuruliFdw {
    // initialise FDW instance
    fn init_instance() {
        let instance = Self::default();
        unsafe {
            INSTANCE = Box::leak(Box::new(instance));
        }
    }

    fn this_mut() -> &'static mut Self {
        unsafe { &mut (*INSTANCE) }
    }

    fn list_rows(&self) -> Result<(), String> {
        let this = Self::this_mut();

        this.src_rows.clear();
        this.src_idx = 0;

        let host = &this.base_url;
        let api_key = &this.api_key;
        let cid = &this.cid;
        let object = &this.object;

        let url = format!("{}/fdw/connections/{}/tables/{}/rows", host, cid, object);
        report_warning(&url);

        report_warning(format!("key {:#?}", api_key).as_str());

        let headers: Vec<(String, String)> = vec![
            ("user-agent".to_owned(), "Huruli FDW".to_owned()),
            ("authorization".to_owned(), format!("Bearer {}", api_key)),
        ];

        let req_body = ListRowsRequest {};

        let req = http::Request {
            method: http::Method::Post,
            url,
            headers,
            body: serde_json::to_string(&req_body).unwrap(),
        };

        let resp = http::post(&req)?;
        report_warning(format!("{:#?}", &resp.body).as_str());
        let resp_json =
            serde_json::from_str::<ListRowsResponse>(&resp.body).map_err(|e| e.to_string())?;

        println!("list_rows response: {:?}", resp_json);

        this.src_rows = resp_json
            .rows
            .iter()
            .filter_map(|v| v.get(0))
            .filter_map(|value| match value {
                Value::String(s) => Some(s.clone()),
                _ => Some(value.to_string()),
            })
            .collect();

        Ok(())
    }

    fn get_row(&self, row_id: &str, columns: &Vec<String>) -> Result<GetRowResponse, String> {
        let this = Self::this_mut();

        let host = &this.base_url;
        let api_key = &this.api_key;
        let cid = &this.cid;
        let object = &this.object;

        let url = format!(
            "{}/fdw/connections/{}/tables/{}/rows/{}",
            host, cid, object, row_id
        );
        report_warning(format!("key {:#?}", api_key).as_str());

        let headers: Vec<(String, String)> = vec![
            ("user-agent".to_owned(), "Huruli FDW".to_owned()),
            ("authorization".to_owned(), format!("Bearer {}", api_key)),
        ];

        let req_body = GetRowRequest {
            cid: cid.to_owned(),
            tableName: this.object.to_owned(),
            rowId: row_id.to_owned(),
            columns: columns.clone(),
        };

        let req = http::Request {
            method: http::Method::Post,
            url,
            headers,
            body: serde_json::to_string(&req_body).unwrap(),
        };

        let resp = http::post(&req)?;

        let resp_json =
            serde_json::from_str::<GetRowResponse>(&resp.body).map_err(|e| e.to_string())?;
        println!("Row response {:#?}", resp_json);

        Ok(resp_json)
    }

    fn map_value_to_cell(&self, type_oid: TypeOid, value: &Value) -> Option<Cell> {
        report_warning(format!("val {:#?} - {:#?}", type_oid, value).as_str());
        let cell = match type_oid {
            TypeOid::Bool => value.as_bool().map(Cell::Bool),
            TypeOid::String => value.as_str().map(|v| Cell::String(v.to_owned())),
            TypeOid::I32 => str_to_i64(value).map(|v| Cell::I32(v as i32)),
            TypeOid::I64 => str_to_i64(value).map(|v| Cell::I64(v as i64)),
            TypeOid::Timestamp => str_to_i64(value).map(|v| Cell::Timestamp(v * 1000)),
            TypeOid::Json => value.as_object().map(|_| Cell::Json(value.to_string())),
            _ => {
                return None;
            }
        };

        return cell;
    }
}

impl Guest for HuruliFdw {
    fn host_version_requirement() -> String {
        // semver expression for Wasm FDW host version requirement
        // ref: https://docs.rs/semver/latest/semver/enum.Op.html
        "^0.1.0".to_string()
    }

    fn init(ctx: &Context) -> FdwResult {
        Self::init_instance();
        let this = Self::this_mut();

        let opts = ctx.get_options(OptionsType::Server);
        this.base_url = opts.require_or("api_url", "https://fdw.huruli.dev");
        this.api_key = opts.require_or("api_key", "");
        this.cid = opts.require_or("connection_id", "cid1");
        this.object = opts.require_or("object", "");
        this.src_rows = Vec::new();
        this.src_idx = 0;

        Ok(())
    }

    fn begin_scan(ctx: &Context) -> FdwResult {
        let this = Self::this_mut();

        let opts = ctx.get_options(OptionsType::Table);
        this.base_url = opts.require_or("api_url", this.base_url.as_str());
        this.api_key = opts.require_or("api_key", this.api_key.as_str());
        this.cid = opts.require_or("connection_id", this.cid.as_str());
        this.object = opts.require_or("object", this.object.as_str());

        this.list_rows()?;

        utils::report_info(&format!(
            "We got response array length: {}",
            this.src_rows.len()
        ));

        Ok(())
    }

    fn iter_scan(ctx: &Context, row: &Row) -> Result<Option<u32>, FdwError> {
        let this = Self::this_mut();

        if this.src_idx >= this.src_rows.len() {
            return Ok(None);
        }

        let row_id = &this.src_rows[this.src_idx];

        let resp = this.get_row(
            row_id,
            &ctx.get_columns()
                .iter()
                .map(|c| c.name())
                .collect::<Vec<String>>(),
        )?;

        for tgt_col in ctx.get_columns() {
            let tgt_col_name = tgt_col.name();

            let col_idx = resp.columns.iter().position(|v| v.eq(&tgt_col_name));

            if let Some(col_idx) = col_idx {
                let src = &resp.values[col_idx];
                row.push(this.map_value_to_cell(tgt_col.type_oid(), src).as_ref());
            } else {
                row.push(None);
            }
        }

        this.src_idx += 1;

        Ok(Some(0))
    }

    fn re_scan(_ctx: &Context) -> FdwResult {
        Err("re_scan on foreign table is not supported".to_owned())
    }

    fn end_scan(_ctx: &Context) -> FdwResult {
        let this = Self::this_mut();
        this.src_rows.clear();
        Ok(())
    }

    fn begin_modify(_ctx: &Context) -> FdwResult {
        Err("modify on foreign table is not supported".to_owned())
    }

    fn insert(_ctx: &Context, _row: &Row) -> FdwResult {
        Ok(())
    }

    fn update(_ctx: &Context, _rowid: Cell, _row: &Row) -> FdwResult {
        Ok(())
    }

    fn delete(_ctx: &Context, _rowid: Cell) -> FdwResult {
        Ok(())
    }

    fn end_modify(_ctx: &Context) -> FdwResult {
        Ok(())
    }
}

bindings::export!(HuruliFdw with_types_in bindings);
