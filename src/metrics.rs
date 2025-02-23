use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default, Serialize)]
pub struct OperationMetrics {
    pub total_operations: u64,
    pub failed_operations: u64,
    #[serde(with = "duration_serde")]
    pub total_processing_time: Duration,
    #[serde(with = "duration_serde")]
    pub max_processing_time: Duration,
    #[serde(with = "option_duration_serde")]
    pub min_processing_time: Option<Duration>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ResourceMetrics {
    pub memory_usage: usize,
    pub operation_queue_size: usize,
    pub peak_memory_usage: usize,
    pub peak_queue_size: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActorMetrics {
    pub operation_metrics: OperationMetrics,
    pub resource_metrics: ResourceMetrics,
    #[serde(with = "option_timestamp_serde")]
    pub last_update: Option<SystemTime>,
    pub uptime_secs: u64,
    #[serde(with = "timestamp_serde")]
    pub start_time: SystemTime,
}

impl Default for ActorMetrics {
    fn default() -> Self {
        Self {
            operation_metrics: OperationMetrics::default(),
            resource_metrics: ResourceMetrics::default(),
            last_update: None,
            uptime_secs: 0,
            start_time: SystemTime::now(),
        }
    }
}

mod timestamp_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

    #[allow(unused)]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let timestamp = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_secs(timestamp))
    }
}

mod option_timestamp_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &Option<SystemTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match time {
            Some(t) => {
                let timestamp = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                serializer.serialize_some(&timestamp)
            }
            None => serializer.serialize_none(),
        }
    }

    #[allow(unused)]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<SystemTime>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let timestamp: Option<u64> = Option::deserialize(deserializer)?;
        Ok(timestamp.map(|t| UNIX_EPOCH + Duration::from_secs(t)))
    }
}

mod duration_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_nanos() as u64)
    }

    #[allow(unused)]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let nanos = u64::deserialize(deserializer)?;
        Ok(Duration::from_nanos(nanos))
    }
}

mod option_duration_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match duration {
            Some(d) => serializer.serialize_some(&d.as_nanos()),
            None => serializer.serialize_none(),
        }
    }

    #[allow(unused)]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let nanos: Option<u64> = Option::deserialize(deserializer)?;
        Ok(nanos.map(Duration::from_nanos))
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
        metrics.operation_metrics.max_processing_time =
            metrics.operation_metrics.max_processing_time.max(duration);
        metrics.operation_metrics.min_processing_time = Some(
            metrics
                .operation_metrics
                .min_processing_time
                .map_or(duration, |min| min.min(duration)),
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
        metrics.resource_metrics.peak_memory_usage =
            metrics.resource_metrics.peak_memory_usage.max(memory);
        metrics.resource_metrics.peak_queue_size =
            metrics.resource_metrics.peak_queue_size.max(queue_size);
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
                op_metrics
                    .total_processing_time
                    .div_f64(op_metrics.total_operations as f64)
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
