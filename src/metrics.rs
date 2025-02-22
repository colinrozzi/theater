use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct OperationMetrics {
    total_operations: u64,
    failed_operations: u64,
    total_processing_time: Duration,
    max_processing_time: Duration,
    min_processing_time: Option<Duration>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ResourceMetrics {
    memory_usage: usize,
    operation_queue_size: usize,
    peak_memory_usage: usize,
    peak_queue_size: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ActorMetrics {
    pub operation_metrics: OperationMetrics,
    pub resource_metrics: ResourceMetrics,
    #[serde(with = "timestamp_serde")]
    pub last_update: Option<SystemTime>,
    pub uptime_secs: u64,
    #[serde(with = "timestamp_serde")]
    pub start_time: SystemTime,
}

mod timestamp_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let timestamp = time
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        serializer.serialize_u64(timestamp)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let timestamp = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_secs(timestamp))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationStats {
    pub success_rate: f64,
    pub avg_processing_time: Duration,
    pub operations_per_second: f64,
    pub total_operations: u64,
    pub failed_operations: u64,
}

#[derive(Debug, Clone)]
pub struct MetricsCollector {
    metrics: Arc<RwLock<ActorMetrics>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(ActorMetrics {
                start_time: SystemTime::now(),
                ..Default::default()
            })),
        }
    }

    pub async fn record_operation(&self, duration: Duration, success: bool) {
        let mut metrics = self.metrics.write().await;
        metrics.operation_metrics.total_operations += 1;
        if !success {
            metrics.operation_metrics.failed_operations += 1;
        }
        metrics.operation_metrics.total_processing_time += duration;
        metrics.operation_metrics.max_processing_time = metrics.operation_metrics.max_processing_time.max(duration);
        metrics.operation_metrics.min_processing_time = Some(
            metrics.operation_metrics.min_processing_time
                .map_or(duration, |min| min.min(duration))
        );
        metrics.last_update = Some(SystemTime::now());
        metrics.uptime_secs = SystemTime::now()
            .duration_since(metrics.start_time)
            .unwrap_or_default()
            .as_secs();
    }

    pub async fn update_resource_usage(&self, memory: usize, queue_size: usize) {
        let mut metrics = self.metrics.write().await;
        metrics.resource_metrics.memory_usage = memory;
        metrics.resource_metrics.operation_queue_size = queue_size;
        metrics.resource_metrics.peak_memory_usage = metrics.resource_metrics.peak_memory_usage.max(memory);
        metrics.resource_metrics.peak_queue_size = metrics.resource_metrics.peak_queue_size.max(queue_size);
        metrics.last_update = Some(SystemTime::now());
        metrics.uptime_secs = SystemTime::now()
            .duration_since(metrics.start_time)
            .unwrap_or_default()
            .as_secs();
    }

    pub async fn get_metrics(&self) -> ActorMetrics {
        let mut metrics = self.metrics.write().await;
        metrics.uptime_secs = SystemTime::now()
            .duration_since(metrics.start_time)
            .unwrap_or_default()
            .as_secs();
        metrics.clone()
    }

    pub async fn get_operation_stats(&self) -> OperationStats {
        let metrics = self.metrics.read().await;
        let op_metrics = &metrics.operation_metrics;
        
        OperationStats {
            success_rate: if op_metrics.total_operations > 0 {
                ((op_metrics.total_operations - op_metrics.failed_operations) as f64
                    / op_metrics.total_operations as f64)
                    * 100.0
            } else {
                0.0
            },
            avg_processing_time: if op_metrics.total_operations > 0 {
                op_metrics.total_processing_time.div_f64(op_metrics.total_operations as f64)
            } else {
                Duration::default()
            },
            operations_per_second: if metrics.uptime_secs > 0 {
                op_metrics.total_operations as f64 / metrics.uptime_secs as f64
            } else {
                0.0
            },
            total_operations: op_metrics.total_operations,
            failed_operations: op_metrics.failed_operations,
        }
    }
}