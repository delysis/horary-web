use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension};

const CHARTS_FILE: &str = "charts.json";
const CHARTS_DB_FILE: &str = "charts.sqlite3";
const SETTINGS_FILE: &str = "settings.json";
const DEFAULT_HOUSE_SYSTEM: &str = "regiomontanus";
const DEFAULT_PLANET_SET: &str = "modern";
const ALLOWED_HOUSE_SYSTEMS: &[&str] = &[
    "placidus",
    "koch",
    "regiomontanus",
    "campanus",
    "equal",
    "wholesign",
    "porphyry",
    "morinus",
    "alcabitius",
    "topocentric",
];
const ALLOWED_PLANET_SETS: &[&str] = &["classical", "modern"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedChart {
    pub id: String,
    pub question: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub chart: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SaveChartRequest {
    pub id: Option<String>,
    pub question: Option<String>,
    pub chart: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedChartSummary {
    pub id: String,
    pub question: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub house_system: String,
    pub planet_set: String,
    pub aspect_settings: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct StorageError {
    pub message: String,
}

impl From<io::Error> for StorageError {
    fn from(value: io::Error) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(value: serde_json::Error) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(value: rusqlite::Error) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

pub type StorageResult<T> = Result<T, StorageError>;

pub fn charts_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CHARTS_FILE)
}

pub fn charts_db_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CHARTS_DB_FILE)
}

pub fn settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(SETTINGS_FILE)
}

pub fn save_chart_to_dir(app_data_dir: &Path, req: SaveChartRequest) -> StorageResult<SavedChart> {
    if !req.chart.is_object() {
        return Err(StorageError {
            message: "chart must be a JSON object".to_string(),
        });
    }

    let conn = open_charts_db(app_data_dir)?;
    let now = unix_timestamp_millis().to_string();
    let id = req.id.unwrap_or_else(|| format!("chart-{now}"));
    let question = normalize_question(req.question);
    let chart_json = serde_json::to_string(&req.chart)?;

    if let Some(created_at) = conn
        .query_row(
            "SELECT created_at FROM charts WHERE id = ?1",
            params![&id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        conn.execute(
            "UPDATE charts SET question = ?2, updated_at = ?3, chart_json = ?4 WHERE id = ?1",
            params![&id, &question, &now, &chart_json],
        )?;
        return Ok(SavedChart {
            id,
            question,
            created_at,
            updated_at: now,
            chart: req.chart,
        });
    }

    let saved = SavedChart {
        id,
        question,
        created_at: now.clone(),
        updated_at: now,
        chart: req.chart,
    };
    conn.execute(
        "INSERT INTO charts (id, question, created_at, updated_at, chart_json) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            &saved.id,
            &saved.question,
            &saved.created_at,
            &saved.updated_at,
            &chart_json
        ],
    )?;
    Ok(saved)
}

pub fn list_charts_from_dir(app_data_dir: &Path) -> StorageResult<Vec<SavedChartSummary>> {
    let conn = open_charts_db(app_data_dir)?;
    let mut statement = conn.prepare(
        "SELECT id, question, created_at, updated_at FROM charts ORDER BY updated_at DESC, created_at DESC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(SavedChartSummary {
            id: row.get(0)?,
            question: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
        })
    })?;

    let mut charts = Vec::new();
    for row in rows {
        charts.push(row?);
    }
    Ok(charts)
}

pub fn get_chart_from_dir(app_data_dir: &Path, id: &str) -> StorageResult<Option<SavedChart>> {
    let conn = open_charts_db(app_data_dir)?;
    let row = conn
        .query_row(
            "SELECT id, question, created_at, updated_at, chart_json FROM charts WHERE id = ?1",
            params![id],
            chart_from_row,
        )
        .optional()?;
    Ok(row)
}

pub fn delete_chart_from_dir(app_data_dir: &Path, id: &str) -> StorageResult<bool> {
    let conn = open_charts_db(app_data_dir)?;
    Ok(conn.execute("DELETE FROM charts WHERE id = ?1", params![id])? > 0)
}

pub fn default_settings() -> AppSettings {
    AppSettings {
        house_system: DEFAULT_HOUSE_SYSTEM.to_string(),
        planet_set: DEFAULT_PLANET_SET.to_string(),
        aspect_settings: None,
    }
}

pub fn load_settings_from_dir(app_data_dir: &Path) -> StorageResult<AppSettings> {
    let path = settings_path(app_data_dir);
    match fs::read_to_string(path) {
        Ok(contents) => {
            let settings = serde_json::from_str(&contents)?;
            validate_settings(settings)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(default_settings()),
        Err(error) => Err(error.into()),
    }
}

pub fn save_settings_to_dir(
    app_data_dir: &Path,
    settings: AppSettings,
) -> StorageResult<AppSettings> {
    let settings = validate_settings(settings)?;
    fs::create_dir_all(app_data_dir)?;
    let contents = serde_json::to_string_pretty(&settings)?;
    fs::write(settings_path(app_data_dir), contents)?;
    Ok(settings)
}

fn read_charts_file(path: &Path) -> StorageResult<Vec<SavedChart>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(serde_json::from_str(&contents)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
fn write_charts_file(path: &Path, charts: &[SavedChart]) -> StorageResult<()> {
    let contents = serde_json::to_string_pretty(charts)?;
    fs::write(path, contents)?;
    Ok(())
}

fn open_charts_db(app_data_dir: &Path) -> StorageResult<Connection> {
    fs::create_dir_all(app_data_dir)?;
    let db_path = charts_db_path(app_data_dir);
    let conn = Connection::open(db_path)?;
    initialize_charts_db(&conn)?;
    migrate_legacy_charts_file(app_data_dir, &conn)?;
    Ok(conn)
}

fn initialize_charts_db(conn: &Connection) -> StorageResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS charts (
            id TEXT PRIMARY KEY NOT NULL,
            question TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            chart_json TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_charts_updated_at ON charts(updated_at DESC, created_at DESC);",
    )?;
    Ok(())
}

fn migrate_legacy_charts_file(app_data_dir: &Path, conn: &Connection) -> StorageResult<()> {
    if chart_count(conn)? > 0 {
        return Ok(());
    }

    let legacy_path = charts_path(app_data_dir);
    if !legacy_path.exists() {
        return Ok(());
    }

    for chart in read_charts_file(&legacy_path)? {
        conn.execute(
            "INSERT OR IGNORE INTO charts (id, question, created_at, updated_at, chart_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                chart.id,
                chart.question,
                chart.created_at,
                chart.updated_at,
                serde_json::to_string(&chart.chart)?
            ],
        )?;
    }
    Ok(())
}

fn chart_count(conn: &Connection) -> StorageResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM charts", [], |row| row.get(0))?)
}

fn chart_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedChart> {
    let chart_json: String = row.get(4)?;
    let chart = serde_json::from_str(&chart_json)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    Ok(SavedChart {
        id: row.get(0)?,
        question: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        chart,
    })
}

fn normalize_question(question: Option<String>) -> Option<String> {
    question
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_settings(settings: AppSettings) -> StorageResult<AppSettings> {
    let house_system = settings.house_system.trim().to_string();
    let planet_set = settings.planet_set.trim().to_string();

    if !ALLOWED_HOUSE_SYSTEMS.contains(&house_system.as_str()) {
        return Err(StorageError {
            message: format!("unsupported house system: {house_system}"),
        });
    }
    if !ALLOWED_PLANET_SETS.contains(&planet_set.as_str()) {
        return Err(StorageError {
            message: format!("unsupported planet set: {planet_set}"),
        });
    }
    if let Some(aspect_settings) = &settings.aspect_settings {
        if !aspect_settings.is_object() {
            return Err(StorageError {
                message: "aspect settings must be a JSON object".to_string(),
            });
        }
    }

    Ok(AppSettings {
        house_system,
        planet_set,
        aspect_settings: settings.aspect_settings,
    })
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_storage_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horary-storage-test-{name}-{}",
            unix_timestamp_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_and_list_chart_summary() {
        let dir = temp_storage_dir("save-list");
        let saved = save_chart_to_dir(
            &dir,
            SaveChartRequest {
                id: Some("contract".to_string()),
                question: Some("  Will I get the contract?  ".to_string()),
                chart: serde_json::json!({ "ascendant": { "sign": "Libra" } }),
            },
        )
        .unwrap();
        let summaries = list_charts_from_dir(&dir).unwrap();

        assert_eq!(saved.id, "contract");
        assert_eq!(saved.question, Some("Will I get the contract?".to_string()));
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "contract");
    }

    #[test]
    fn update_existing_chart_preserves_created_at() {
        let dir = temp_storage_dir("update");
        let first = save_chart_to_dir(
            &dir,
            SaveChartRequest {
                id: Some("same".to_string()),
                question: Some("First".to_string()),
                chart: serde_json::json!({ "version": 1 }),
            },
        )
        .unwrap();
        let second = save_chart_to_dir(
            &dir,
            SaveChartRequest {
                id: Some("same".to_string()),
                question: Some("Second".to_string()),
                chart: serde_json::json!({ "version": 2 }),
            },
        )
        .unwrap();
        let charts = list_charts_from_dir(&dir).unwrap();
        let loaded = get_chart_from_dir(&dir, "same").unwrap().unwrap();

        assert_eq!(charts.len(), 1);
        assert_eq!(second.created_at, first.created_at);
        assert_eq!(second.question, Some("Second".to_string()));
        assert_eq!(loaded.chart["version"], 2);
    }

    #[test]
    fn get_and_delete_chart() {
        let dir = temp_storage_dir("get-delete");
        save_chart_to_dir(
            &dir,
            SaveChartRequest {
                id: Some("delete-me".to_string()),
                question: None,
                chart: serde_json::json!({ "moon": "Capricorn" }),
            },
        )
        .unwrap();

        assert!(get_chart_from_dir(&dir, "delete-me").unwrap().is_some());
        assert!(delete_chart_from_dir(&dir, "delete-me").unwrap());
        assert!(get_chart_from_dir(&dir, "delete-me").unwrap().is_none());
        assert!(!delete_chart_from_dir(&dir, "delete-me").unwrap());
    }

    #[test]
    fn rejects_non_object_chart_payload() {
        let dir = temp_storage_dir("reject");
        let error = save_chart_to_dir(
            &dir,
            SaveChartRequest {
                id: None,
                question: None,
                chart: serde_json::json!("not an object"),
            },
        )
        .unwrap_err();

        assert_eq!(error.message, "chart must be a JSON object");
    }

    #[test]
    fn chart_history_uses_sqlite_database() {
        let dir = temp_storage_dir("sqlite");
        assert!(!charts_db_path(&dir).exists());

        save_chart_to_dir(
            &dir,
            SaveChartRequest {
                id: Some("sqlite-chart".to_string()),
                question: Some("Stored where?".to_string()),
                chart: serde_json::json!({ "storage": "sqlite" }),
            },
        )
        .unwrap();

        assert!(charts_db_path(&dir).exists());
        assert!(!charts_path(&dir).exists());
    }

    #[test]
    fn migrates_legacy_charts_json_into_sqlite() {
        let dir = temp_storage_dir("legacy-migration");
        write_charts_file(
            &charts_path(&dir),
            &[SavedChart {
                id: "legacy".to_string(),
                question: Some("Old chart?".to_string()),
                created_at: "1000".to_string(),
                updated_at: "1001".to_string(),
                chart: serde_json::json!({ "source": "json" }),
            }],
        )
        .unwrap();

        let summaries = list_charts_from_dir(&dir).unwrap();
        let chart = get_chart_from_dir(&dir, "legacy").unwrap().unwrap();

        assert!(charts_db_path(&dir).exists());
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "legacy");
        assert_eq!(chart.chart["source"], "json");
    }

    #[test]
    fn loads_default_settings_when_missing() {
        let dir = temp_storage_dir("settings-default");
        let settings = load_settings_from_dir(&dir).unwrap();

        assert_eq!(settings, default_settings());
    }

    #[test]
    fn saves_and_loads_settings() {
        let dir = temp_storage_dir("settings-roundtrip");
        let saved = save_settings_to_dir(
            &dir,
            AppSettings {
                house_system: "campanus".to_string(),
                planet_set: "classical".to_string(),
                aspect_settings: Some(serde_json::json!({
                    "sextile": {
                        "enabled": false,
                        "orb": 2.5
                    }
                })),
            },
        )
        .unwrap();
        let loaded = load_settings_from_dir(&dir).unwrap();

        assert_eq!(saved.house_system, "campanus");
        assert_eq!(saved.planet_set, "classical");
        assert_eq!(
            saved.aspect_settings.as_ref().unwrap()["sextile"]["orb"],
            2.5
        );
        assert_eq!(loaded, saved);
    }

    #[test]
    fn rejects_invalid_settings_values() {
        let dir = temp_storage_dir("settings-invalid");
        let error = save_settings_to_dir(
            &dir,
            AppSettings {
                house_system: "unknown".to_string(),
                planet_set: "modern".to_string(),
                aspect_settings: None,
            },
        )
        .unwrap_err();

        assert_eq!(error.message, "unsupported house system: unknown");
        assert!(!settings_path(&dir).exists());
    }

    #[test]
    fn rejects_invalid_aspect_settings_values() {
        let dir = temp_storage_dir("settings-invalid-aspects");
        let error = save_settings_to_dir(
            &dir,
            AppSettings {
                house_system: "regiomontanus".to_string(),
                planet_set: "modern".to_string(),
                aspect_settings: Some(serde_json::json!("bad")),
            },
        )
        .unwrap_err();

        assert_eq!(error.message, "aspect settings must be a JSON object");
        assert!(!settings_path(&dir).exists());
    }
}
