use irodori_connector_abi::{option_bool, option_string, push_sensitive, request_containers};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use iotdb::client::remote::{Config, RpcSession};
use iotdb::client::{DataSet, RowRecord, Session, Value as IotValue};
use serde_json::{json, Map, Value};

use crate::abi::{self, IrodoriConnectorBuffer};
use crate::{ABI_VERSION, CONFIG_JSON, DRIVER_LINKED, ENGINE, MANIFEST_JSON};

thread_local! {
    static CONNECTIONS: RefCell<HashMap<String, IotdbConnection>> = RefCell::new(HashMap::new());
}

struct IotdbConnection {
    session: RpcSession,
    config: IotdbConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IotdbConfig {
    target: String,
    database: Option<String>,
    user: String,
    timeout_ms: i64,
    fetch_size: i32,
    redaction_values: Vec<String>,
}

#[derive(Default)]
struct ObjectMeta {
    columns: Vec<Value>,
}

type QueryRows = Vec<Vec<Value>>;
type QueryOutput = (Vec<String>, QueryRows, bool);

pub fn call_json(request: IrodoriConnectorBuffer) -> IrodoriConnectorBuffer {
    let request = match abi::parse_request(request) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let method = match abi::request_method(request.as_ref()) {
        Ok(method) => method,
        Err(response) => return response,
    };

    match method {
        "health" | "ping" => abi::ok(Map::from_iter([
            ("engine".to_string(), Value::String(ENGINE.to_string())),
            ("abiVersion".to_string(), json!(ABI_VERSION)),
            ("driverLinked".to_string(), Value::Bool(DRIVER_LINKED)),
        ])),
        "describe" | "capabilities" => abi::ok(Map::from_iter([
            ("engine".to_string(), Value::String(ENGINE.to_string())),
            ("abiVersion".to_string(), json!(ABI_VERSION)),
            ("driverLinked".to_string(), Value::Bool(DRIVER_LINKED)),
            (
                "manifest".to_string(),
                serde_json::from_str(MANIFEST_JSON).unwrap_or(Value::Null),
            ),
            (
                "config".to_string(),
                serde_json::from_str(CONFIG_JSON).unwrap_or(Value::Null),
            ),
        ])),
        "manifest" => abi::owned_buffer(MANIFEST_JSON.to_string()),
        "config" => abi::owned_buffer(CONFIG_JSON.to_string()),
        "connect" => connect(request.as_ref().expect("connect has request")),
        "query" => query(request.as_ref().expect("query has request")),
        "metadata" => metadata(request.as_ref().expect("metadata has request")),
        "close" => close(request.as_ref().expect("close has request")),
        other => abi::error(
            "connector.unknownMethod",
            format!("unknown connector method: {other}"),
        ),
    }
}

fn connect(request: &Value) -> IrodoriConnectorBuffer {
    let connection_id = abi::connection_id(Some(request));
    let (client_config, connector_config) = match IotdbConfig::from_request(request) {
        Ok(config) => config,
        Err(err) => return abi::error("connector.invalidRequest", err),
    };
    let mut session = match RpcSession::new(client_config) {
        Ok(session) => session,
        Err(err) => {
            return abi::error(
                "connector.connectFailed",
                connector_config.redact(&err.to_string()),
            )
        }
    };
    if let Err(err) = session.open() {
        return abi::error(
            "connector.connectFailed",
            connector_config.redact(&err.to_string()),
        );
    }
    let server_version = run_scalar(&mut session, "show version", connector_config.timeout_ms)
        .unwrap_or_else(|_| "Apache IoTDB".to_string());
    CONNECTIONS.with(|connections| {
        connections.borrow_mut().insert(
            connection_id.clone(),
            IotdbConnection {
                session,
                config: connector_config.clone(),
            },
        );
    });
    abi::ok(Map::from_iter([
        ("engine".to_string(), Value::String(ENGINE.to_string())),
        ("connectionId".to_string(), Value::String(connection_id)),
        ("driverLinked".to_string(), Value::Bool(DRIVER_LINKED)),
        (
            "database".to_string(),
            connector_config
                .database
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        ),
        ("user".to_string(), Value::String(connector_config.user)),
        ("serverVersion".to_string(), Value::String(server_version)),
    ]))
}

fn query(request: &Value) -> IrodoriConnectorBuffer {
    let connection_id = abi::connection_id(Some(request));
    let Some(sql) = abi::string_field(request, "sql")
        .or_else(|| abi::string_field(request, "query"))
        .or_else(|| abi::string_field(request, "statement"))
    else {
        return abi::error(
            "connector.invalidRequest",
            "query requires a string sql, query, or statement field.",
        );
    };
    let max_rows = abi::max_rows(request);
    let result = with_connection_mut(&connection_id, |connection| {
        let config = connection.config.clone();
        (run_query(connection, sql, max_rows), config)
    });
    match result {
        Ok((Ok((columns, rows, truncated)), _)) => abi::ok(Map::from_iter([
            ("connectionId".to_string(), Value::String(connection_id)),
            (
                "columns".to_string(),
                Value::Array(columns.into_iter().map(Value::String).collect()),
            ),
            (
                "rows".to_string(),
                Value::Array(rows.into_iter().map(Value::Array).collect()),
            ),
            ("truncated".to_string(), Value::Bool(truncated)),
        ])),
        Ok((Err(err), config)) => abi::error("connector.queryFailed", config.redact(&err)),
        Err(response) => response,
    }
}

fn metadata(request: &Value) -> IrodoriConnectorBuffer {
    let connection_id = abi::connection_id(Some(request));
    let result = with_connection_mut(&connection_id, |connection| {
        let config = connection.config.clone();
        (load_metadata(connection), config)
    });
    match result {
        Ok((Ok(metadata), _)) => abi::ok(Map::from_iter([
            ("connectionId".to_string(), Value::String(connection_id)),
            ("metadata".to_string(), metadata),
        ])),
        Ok((Err(err), config)) => abi::error("connector.metadataFailed", config.redact(&err)),
        Err(response) => response,
    }
}

fn close(request: &Value) -> IrodoriConnectorBuffer {
    let connection_id = abi::connection_id(Some(request));
    let removed = CONNECTIONS.with(|connections| connections.borrow_mut().remove(&connection_id));
    let closed = removed.is_some();
    if let Some(mut connection) = removed {
        let _ = connection.session.close();
    }
    abi::ok(Map::from_iter([
        ("connectionId".to_string(), Value::String(connection_id)),
        ("closed".to_string(), Value::Bool(closed)),
    ]))
}

impl IotdbConfig {
    fn from_request(request: &Value) -> Result<(Config, Self), String> {
        let url = option_string(request, &["url", "connectionString", "dsn"]);
        let parsed = url.as_deref().and_then(parse_iotdb_url);
        let host = option_string(request, &["host"])
            .or_else(|| parsed.as_ref().map(|parsed| parsed.host.clone()))
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port = option_i32(request, &["port"])
            .or_else(|| {
                parsed
                    .as_ref()
                    .and_then(|parsed| parsed.port.map(i32::from))
            })
            .unwrap_or(6667);
        let database = option_string(request, &["database", "db"])
            .or_else(|| parsed.as_ref().and_then(|parsed| parsed.database.clone()));
        let user = option_string(request, &["user", "username"])
            .or_else(|| parsed.as_ref().and_then(|parsed| parsed.user.clone()))
            .unwrap_or_else(|| "root".to_string());
        let password = option_string(request, &["password"])
            .or_else(|| parsed.as_ref().and_then(|parsed| parsed.password.clone()))
            .unwrap_or_else(|| "root".to_string());
        let timeout_ms = option_i64(request, &["timeoutMs", "queryTimeoutMs"]).unwrap_or(30_000);
        let fetch_size = option_i32(request, &["fetchSize"]).unwrap_or(1_000).max(1);
        let timezone = option_string(request, &["timezone", "timeZone"]);
        let enable_compression =
            option_bool(request, &["enableCompression", "compression"]).unwrap_or(false);
        let config = Config {
            host: host.clone(),
            port,
            username: user.clone(),
            password: password.clone(),
            timeout_ms: Some(timeout_ms),
            fetch_size,
            timezone,
            enable_compression,
            ..Config::default()
        };
        let mut redaction_values = Vec::new();
        push_sensitive(&mut redaction_values, Some(&password));
        Ok((
            config,
            Self {
                target: format!("{host}:{port}"),
                database,
                user,
                timeout_ms,
                fetch_size,
                redaction_values,
            },
        ))
    }

    fn redact(&self, message: &str) -> String {
        self.redaction_values
            .iter()
            .fold(message.to_string(), |message, secret| {
                if secret.is_empty() {
                    message
                } else {
                    message.replace(secret, "****")
                }
            })
    }
}

fn run_query(
    connection: &mut IotdbConnection,
    sql: &str,
    cap: usize,
) -> Result<QueryOutput, String> {
    let lower = sql.trim_start().to_ascii_lowercase();
    let update = starts_with_any(
        &lower,
        &[
            "insert", "delete", "create", "drop", "alter", "set", "flush", "load", "unload",
        ],
    );
    let mut dataset = if update {
        match connection
            .session
            .execute_update_statement(sql)
            .map_err(|err| format!("IoTDB update failed: {err}"))?
        {
            Some(dataset) => dataset,
            None => return Ok((Vec::new(), Vec::new(), false)),
        }
    } else {
        connection
            .session
            .execute_query_statement(sql, Some(connection.config.timeout_ms))
            .map_err(|err| format!("IoTDB query failed: {err}"))?
    };
    drain_dataset(dataset.as_mut(), cap)
}

fn drain_dataset(dataset: &mut dyn DataSet, cap: usize) -> Result<QueryOutput, String> {
    let ignore_timestamp = dataset.is_ignore_timestamp();
    let mut columns = dataset.get_column_names();
    if !ignore_timestamp {
        columns.insert(0, "Time".to_string());
    }
    let mut rows = Vec::new();
    while rows.len() < cap {
        let Some(record) = dataset.next() else {
            break;
        };
        rows.push(row_record_to_json(record, ignore_timestamp));
    }
    let truncated = if rows.len() == cap {
        dataset.next().is_some()
    } else {
        false
    };
    Ok((columns, rows, truncated))
}

fn row_record_to_json(record: RowRecord, ignore_timestamp: bool) -> Vec<Value> {
    let mut row = Vec::new();
    if !ignore_timestamp {
        row.push(json!(record.timestamp));
    }
    row.extend(record.values.into_iter().map(iot_value_to_json));
    row
}

fn run_scalar(session: &mut RpcSession, sql: &str, timeout_ms: i64) -> Result<String, String> {
    let mut dataset = session
        .execute_query_statement(sql, Some(timeout_ms))
        .map_err(|err| format!("IoTDB scalar query failed: {err}"))?;
    Ok(dataset
        .next()
        .and_then(|record| record.values.into_iter().next())
        .map(iot_value_to_json)
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            Value::Number(value) => Some(value.to_string()),
            Value::Bool(value) => Some(value.to_string()),
            _ => None,
        })
        .unwrap_or_default())
}

fn load_metadata(connection: &mut IotdbConnection) -> Result<Value, String> {
    let (columns, rows, _) = run_query(connection, "show timeseries root.**", 100_000)
        .or_else(|_| run_query(connection, "show timeseries", 100_000))?;
    let mut objects = BTreeMap::<(String, String), ObjectMeta>::new();
    for row in rows {
        let timeseries = field(
            &columns,
            &row,
            &["Timeseries", "timeseries", "timeseriesPath"],
        )
        .or_else(|| row.first().and_then(json_to_string))
        .unwrap_or_default();
        if timeseries.is_empty() {
            continue;
        }
        let (schema, device, measurement) = split_timeseries(
            &timeseries,
            field(
                &columns,
                &row,
                &["Database", "database", "Storage Group", "storage group"],
            )
            .as_deref(),
        );
        let object = objects.entry((schema.clone(), device.clone())).or_default();
        object.columns.push(json!({
            "name": measurement,
            "path": timeseries,
            "dataType": field(&columns, &row, &["DataType", "dataType", "Type", "type"]).unwrap_or_default(),
            "encoding": field(&columns, &row, &["Encoding", "encoding"]),
            "compression": field(&columns, &row, &["Compression", "compression"]),
            "alias": field(&columns, &row, &["Alias", "alias"])
        }));
    }
    let mut schemas = BTreeMap::<String, Vec<Value>>::new();
    for ((schema, device), object) in objects {
        schemas.entry(schema.clone()).or_default().push(json!({
            "schema": schema,
            "name": device,
            "kind": "device",
            "columns": object.columns,
            "indexes": [],
            "primaryKey": ["Time"],
            "foreignKeys": []
        }));
    }
    Ok(json!({
        "schemas": schemas
            .into_iter()
            .map(|(name, objects)| json!({ "name": name, "objects": objects }))
            .collect::<Vec<_>>()
    }))
}

fn split_timeseries(path: &str, database: Option<&str>) -> (String, String, String) {
    let parts = path
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let measurement = parts.last().copied().unwrap_or(path).to_string();
    let device = if parts.len() > 1 {
        parts[..parts.len() - 1].join(".")
    } else {
        path.to_string()
    };
    let schema = database
        .map(str::to_string)
        .or_else(|| {
            if parts.len() >= 2 {
                Some(format!("{}.{}", parts[0], parts[1]))
            } else {
                parts.first().map(|part| (*part).to_string())
            }
        })
        .unwrap_or_else(|| "root".to_string());
    (schema, device, measurement)
}

fn iot_value_to_json(value: IotValue) -> Value {
    match value {
        IotValue::Bool(value) => Value::Bool(value),
        IotValue::Int32(value) => json!(value),
        IotValue::Int64(value) => json!(value),
        IotValue::Float(value) => json!(value),
        IotValue::Double(value) => json!(value),
        IotValue::Text(value) => Value::String(value),
        IotValue::Null => Value::Null,
    }
}

fn field(columns: &[String], row: &[Value], names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        columns
            .iter()
            .position(|column| column.eq_ignore_ascii_case(name))
            .and_then(|index| row.get(index))
            .and_then(json_to_string)
    })
}

fn json_to_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.trim().to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        other => Some(other.to_string()),
    }
}

#[derive(Debug, Clone)]
struct ParsedIotdbUrl {
    host: String,
    port: Option<u16>,
    database: Option<String>,
    user: Option<String>,
    password: Option<String>,
}

fn parse_iotdb_url(value: &str) -> Option<ParsedIotdbUrl> {
    let rest = value.strip_prefix("iotdb://")?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (userinfo, hostport) = authority
        .rsplit_once('@')
        .map(|(userinfo, hostport)| (Some(userinfo), hostport))
        .unwrap_or((None, authority));
    let (host, port) = hostport
        .rsplit_once(':')
        .map(|(host, port)| (host.to_string(), port.parse::<u16>().ok()))
        .unwrap_or((hostport.to_string(), None));
    let (user, password) = userinfo
        .map(|userinfo| {
            userinfo
                .split_once(':')
                .map(|(user, password)| {
                    (Some(percent_decode(user)), Some(percent_decode(password)))
                })
                .unwrap_or((Some(percent_decode(userinfo)), None))
        })
        .unwrap_or((None, None));
    Some(ParsedIotdbUrl {
        host,
        port,
        database: (!path.is_empty()).then(|| percent_decode(path.trim_start_matches('/'))),
        user,
        password,
    })
}

fn percent_decode(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            if let Ok(byte) = u8::from_str_radix(&format!("{h1}{h2}"), 16) {
                out.push(byte as char);
            }
        } else if ch == '+' {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

fn starts_with_any(value: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| value.starts_with(prefix))
}

fn with_connection_mut<R>(
    connection_id: &str,
    f: impl FnOnce(&mut IotdbConnection) -> R,
) -> Result<R, IrodoriConnectorBuffer> {
    CONNECTIONS.with(|connections| {
        let mut connections = connections.borrow_mut();
        let Some(connection) = connections.get_mut(connection_id) else {
            return Err(abi::error(
                "connector.connectionNotFound",
                format!("no open connection: {connection_id}"),
            ));
        };
        Ok(f(connection))
    })
}

fn option_i32(request: &Value, fields: &[&str]) -> Option<i32> {
    option_i64(request, fields).and_then(|value| i32::try_from(value).ok())
}

fn option_i64(request: &Value, fields: &[&str]) -> Option<i64> {
    request_containers(request)
        .into_iter()
        .find_map(|container| {
            fields.iter().find_map(|field| {
                container
                    .get(*field)
                    .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iotdb_url() {
        let parsed = parse_iotdb_url("iotdb://root:secret@localhost:6667/root.sg").unwrap();
        assert_eq!(parsed.host, "localhost");
        assert_eq!(parsed.port, Some(6667));
        assert_eq!(parsed.database.as_deref(), Some("root.sg"));
        assert_eq!(parsed.user.as_deref(), Some("root"));
        assert_eq!(parsed.password.as_deref(), Some("secret"));
    }

    #[test]
    fn splits_timeseries_path() {
        let (schema, device, measurement) = split_timeseries("root.sg.d1.temp", None);
        assert_eq!(schema, "root.sg");
        assert_eq!(device, "root.sg.d1");
        assert_eq!(measurement, "temp");
    }
}
