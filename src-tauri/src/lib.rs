use ai::{
    cancel_interpretation_stream_from_state, generate_interpretation_from_state,
    start_interpretation_stream_from_state, AiError, AiGenerationState, HoraryInterpretation,
    HoraryInterpretationRequest, InterpretationStreamStarted,
};
use geocode::{
    geocode_with_cache, reverse_geocode_local_city, GeocodeError, GeocodeRequest, GeocodeState,
    LocationCandidate, LocationLabel, ReverseGeocodeRequest,
};
#[cfg(not(feature = "native-llama"))]
use llama::start_llama_with_sidecar_dir;
use llama::{
    get_llama_sidecar_status, get_model_by_id, get_model_status as llama_status_from_state,
    import_model_to_dir, install_llama_sidecar_to_dir, list_models_in_dir, stop_llama_process,
    ImportLlamaSidecarRequest, ImportModelRequest, LlamaError, LlamaSidecarStatus, LlamaState,
    LlamaStatus, ModelInfo, StartLlamaRequest,
};
use model_manifest::{
    bundled_model_manifest, download_manifest_model_to_dir, DownloadModelRequest, ModelManifest,
    ModelManifestError,
};
use native_llama::{native_llama_runtime_info, NativeLlamaRuntimeInfo};
#[cfg(feature = "native-llama")]
use native_llama_worker::start_native_llama_in_dir;
use native_llama_worker::{
    native_llama_health, native_llama_status, stop_native_llama, NativeLlamaHealth,
    NativeLlamaState,
};
use serde::Serialize;
use storage::{
    delete_chart_from_dir, get_chart_from_dir, list_charts_from_dir, load_settings_from_dir,
    save_chart_to_dir, save_settings_to_dir, AppSettings, SaveChartRequest, SavedChart,
    SavedChartSummary, StorageError,
};
use tauri::Manager;

mod ai;
mod geocode;
mod inference;
mod llama;
mod model_manifest;
mod native_llama;
mod native_llama_worker;
mod storage;

#[derive(Serialize)]
struct AppInfo {
    product_name: &'static str,
    version: &'static str,
    runtime: &'static str,
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo {
        product_name: "Whorary",
        version: env!("CARGO_PKG_VERSION"),
        runtime: "tauri",
    }
}

#[tauri::command]
fn save_chart(app: tauri::AppHandle, req: SaveChartRequest) -> Result<SavedChart, StorageError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| StorageError {
        message: error.to_string(),
    })?;
    save_chart_to_dir(&app_data_dir, req)
}

#[tauri::command]
fn list_charts(app: tauri::AppHandle) -> Result<Vec<SavedChartSummary>, StorageError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| StorageError {
        message: error.to_string(),
    })?;
    list_charts_from_dir(&app_data_dir)
}

#[tauri::command]
fn get_chart(app: tauri::AppHandle, id: String) -> Result<Option<SavedChart>, StorageError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| StorageError {
        message: error.to_string(),
    })?;
    get_chart_from_dir(&app_data_dir, &id)
}

#[tauri::command]
fn delete_chart(app: tauri::AppHandle, id: String) -> Result<bool, StorageError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| StorageError {
        message: error.to_string(),
    })?;
    delete_chart_from_dir(&app_data_dir, &id)
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<AppSettings, StorageError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| StorageError {
        message: error.to_string(),
    })?;
    load_settings_from_dir(&app_data_dir)
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    settings: AppSettings,
) -> Result<AppSettings, StorageError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| StorageError {
        message: error.to_string(),
    })?;
    save_settings_to_dir(&app_data_dir, settings)
}

#[tauri::command]
fn geocode_location(
    state: tauri::State<'_, GeocodeState>,
    req: GeocodeRequest,
) -> Result<Vec<LocationCandidate>, GeocodeError> {
    geocode_with_cache(&state, req)
}

#[tauri::command]
fn reverse_geocode_location(
    req: ReverseGeocodeRequest,
) -> Result<Option<LocationLabel>, GeocodeError> {
    reverse_geocode_local_city(req)
}

#[tauri::command]
fn list_models(app: tauri::AppHandle) -> Result<Vec<ModelInfo>, LlamaError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| LlamaError {
        message: error.to_string(),
    })?;
    list_models_in_dir(&app_data_dir)
}

#[tauri::command]
fn import_model(app: tauri::AppHandle, req: ImportModelRequest) -> Result<ModelInfo, LlamaError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| LlamaError {
        message: error.to_string(),
    })?;
    import_model_to_dir(&app_data_dir, req)
}

#[tauri::command]
fn import_llama_sidecar(
    app: tauri::AppHandle,
    req: ImportLlamaSidecarRequest,
) -> Result<LlamaSidecarStatus, LlamaError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| LlamaError {
        message: error.to_string(),
    })?;
    install_llama_sidecar_to_dir(&app_data_dir, req)
}

#[tauri::command]
fn get_model_info(app: tauri::AppHandle, model_id: String) -> Result<ModelInfo, LlamaError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| LlamaError {
        message: error.to_string(),
    })?;
    get_model_by_id(&app_data_dir, &model_id)
}

#[tauri::command]
fn get_model_manifest() -> Result<ModelManifest, ModelManifestError> {
    bundled_model_manifest()
}

#[tauri::command]
async fn download_model(
    app: tauri::AppHandle,
    req: DownloadModelRequest,
) -> Result<ModelInfo, ModelManifestError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| ModelManifestError {
            message: error.to_string(),
        })?;
    download_manifest_model_to_dir(&app_data_dir, req).await
}

#[tauri::command]
fn get_model_status(
    state: tauri::State<'_, LlamaState>,
    native_state: tauri::State<'_, NativeLlamaState>,
) -> Result<LlamaStatus, LlamaError> {
    let native_status = native_llama_status(&native_state)?;
    if native_status.running {
        Ok(native_status)
    } else {
        llama_status_from_state(&state)
    }
}

#[tauri::command]
fn get_llama_sidecar_info(app: tauri::AppHandle) -> Result<LlamaSidecarStatus, LlamaError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| LlamaError {
        message: error.to_string(),
    })?;
    let resource_dir = app.path().resource_dir().ok();
    Ok(get_llama_sidecar_status(
        &app_data_dir,
        resource_dir.as_deref(),
    ))
}

#[tauri::command]
fn get_native_llama_runtime_info() -> NativeLlamaRuntimeInfo {
    native_llama_runtime_info()
}

#[tauri::command]
fn get_native_llama_health(
    state: tauri::State<'_, NativeLlamaState>,
) -> Result<NativeLlamaHealth, LlamaError> {
    native_llama_health(&state)
}

#[tauri::command]
fn start_llama(
    app: tauri::AppHandle,
    state: tauri::State<'_, LlamaState>,
    native_state: tauri::State<'_, NativeLlamaState>,
    req: StartLlamaRequest,
) -> Result<LlamaStatus, LlamaError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| LlamaError {
        message: error.to_string(),
    })?;
    #[cfg(feature = "native-llama")]
    {
        let _ = stop_llama_process(&state);
        start_native_llama_in_dir(&app_data_dir, &native_state, req)
    }
    #[cfg(not(feature = "native-llama"))]
    {
        let _ = native_state;
        let resource_dir = app.path().resource_dir().ok();
        start_llama_with_sidecar_dir(&app_data_dir, resource_dir.as_deref(), &state, req)
    }
}

#[tauri::command]
fn stop_llama(
    state: tauri::State<'_, LlamaState>,
    native_state: tauri::State<'_, NativeLlamaState>,
) -> Result<bool, LlamaError> {
    let native_stopped = stop_native_llama(&native_state)?;
    let sidecar_stopped = stop_llama_process(&state)?;
    Ok(native_stopped || sidecar_stopped)
}

#[tauri::command]
async fn generate_interpretation(
    state: tauri::State<'_, LlamaState>,
    native_state: tauri::State<'_, NativeLlamaState>,
    req: HoraryInterpretationRequest,
) -> Result<HoraryInterpretation, AiError> {
    if native_llama_status(&native_state)?.running {
        return ai::generate_interpretation_from_native_state(&native_state, req);
    }
    generate_interpretation_from_state(&state, req).await
}

#[tauri::command]
async fn start_interpretation_stream(
    app: tauri::AppHandle,
    llama_state: tauri::State<'_, LlamaState>,
    native_state: tauri::State<'_, NativeLlamaState>,
    generation_state: tauri::State<'_, AiGenerationState>,
    req: HoraryInterpretationRequest,
) -> Result<InterpretationStreamStarted, AiError> {
    if native_llama_status(&native_state)?.running {
        return ai::start_interpretation_stream_from_native_state(app, &generation_state, req);
    }
    start_interpretation_stream_from_state(app, &llama_state, &generation_state, req).await
}

#[tauri::command]
fn cancel_interpretation_stream(
    generation_state: tauri::State<'_, AiGenerationState>,
    generation_id: String,
) -> Result<bool, AiError> {
    cancel_interpretation_stream_from_state(&generation_state, generation_id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(LlamaState::default())
        .manage(NativeLlamaState::default())
        .manage(AiGenerationState::default())
        .manage(GeocodeState::default())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            save_chart,
            list_charts,
            get_chart,
            delete_chart,
            get_settings,
            save_settings,
            geocode_location,
            reverse_geocode_location,
            list_models,
            import_model,
            import_llama_sidecar,
            get_model_info,
            get_model_manifest,
            download_model,
            get_model_status,
            get_llama_sidecar_info,
            get_native_llama_runtime_info,
            get_native_llama_health,
            start_llama,
            stop_llama,
            generate_interpretation,
            start_interpretation_stream,
            cancel_interpretation_stream
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
