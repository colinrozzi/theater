use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Default, Serialize)]
pub struct OperationMetrics {
    total_operations: u64,
    failed_operations: u64,
    #[serde(with = "duration_serde")]
    total_processing_time: Duration,
    #[serde(with = "duration_serde")]
    max_processing_time: Duration,
    #[serde(with = "option_duration_serde")]
    min_processing_time: Option<Duration>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ResourceMetrics {
    memory_usage: usize,
    operation_queue_size: usize,
    peak_memory_usage: usize,
    peak_queue_size: usize,
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

mod option_timestamp_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &Option<SystemTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match time {
            Some(t) => {
                let timestamp = t
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                serializer.serialize_some(&timestamp)
            }
            None => serializer.serialize_none(),
        }
    }

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

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let nanos: Option<u64> = Option::deserialize(deserializer)?;
        Ok(nanos.map(Duration::from_nanos))
    }
}

// [Rest of the file remains the same]
