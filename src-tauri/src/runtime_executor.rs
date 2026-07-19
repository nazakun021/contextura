use std::time::Duration;

pub struct RuntimeExecutor;

impl RuntimeExecutor {
    pub async fn ensure_sidecar_ready<R: tauri::Runtime>(
        &self,
        request: crate::sidecar_runtime_adapter::SidecarEnsureRequest<'_, R>,
    ) -> crate::sidecar_runtime_adapter::SidecarEnsureResult {
        let adapter = crate::sidecar_runtime_adapter::SidecarRuntimeAdapter;
        adapter.ensure_ready(request).await
    }

    pub fn idle_sleep_duration(on_battery: bool) -> Duration {
        if on_battery {
            Duration::from_secs(5)
        } else {
            Duration::from_secs(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeExecutor;

    #[test]
    fn idle_sleep_duration_tracks_power_source() {
        assert_eq!(
            RuntimeExecutor::idle_sleep_duration(true),
            Duration::from_secs(5)
        );
        assert_eq!(
            RuntimeExecutor::idle_sleep_duration(false),
            Duration::from_secs(2)
        );
    }

    use std::time::Duration;
}
