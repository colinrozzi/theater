// src/cli/store.rs
use anyhow::{Context, Result};
use clap::Subcommand;
use console::style;
use std::net::SocketAddr;
use tokio::fs;
use tokio::sync::oneshot;

use crate::cli::actor::connect_to_server; // Using the existing connect function
use crate::messages::{StoreCommand, StoreResponse, TheaterCommand};

/// Commands for interacting with the content-addressed store
#[derive(Subcommand, Clone)]
pub enum StoreCommands {
    /// Put content into the store
    Put {
        /// File to store
        #[arg(value_name = "FILE")]
        file: String,

        /// Optional label to assign to content
        #[arg(short, long)]
        label: Option<String>,
    },

    /// Get content from the store
    Get {
        /// Content hash or label to retrieve
        #[arg(value_name = "REFERENCE")]
        reference: String,

        /// Output file (if not provided, content will be printed to stdout)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Label content in the store
    Label {
        /// Label to assign
        #[arg(value_name = "LABEL")]
        label: String,

        /// Content hash to label
        #[arg(value_name = "HASH")]
        hash: String,
    },

    /// List all labels in the store
    ListLabels,

    /// List content with a specific label
    ListByLabel {
        /// Label to list content for
        #[arg(value_name = "LABEL")]
        label: String,
    },

    /// List all content in the store
    ListAll,

    /// Calculate total size of store content
    Size,

    /// Remove a label
    RemoveLabel {
        /// Label to remove
        #[arg(value_name = "LABEL")]
        label: String,
    },

    /// Remove a content reference from a label
    RemoveFromLabel {
        /// Label to remove from
        #[arg(value_name = "LABEL")]
        label: String,

        /// Content hash to remove
        #[arg(value_name = "HASH")]
        hash: String,
    },
}

pub async fn handle_store_command(cmd: &StoreCommands, address: &str) -> Result<()> {
    // Connect to the Theater server using the existing connect function
    let connection = connect_to_server(address).await?;

    match cmd {
        StoreCommands::Put { file, label } => {
            let content = fs::read(file)
                .await
                .context(format!("Failed to read file {}", file))?;

            if let Some(label_str) = label {
                // Put content with a label
                let (tx, rx) = oneshot::channel();

                connection
                    .send(TheaterCommand::StoreOperation {
                        command: StoreCommand::PutAtLabel {
                            label: label_str.clone(),
                            content,
                        },
                        response_tx: tx,
                    })
                    .await?;

                match rx.await? {
                    StoreResponse::ContentRef(hash) => {
                        println!(
                            "Content stored with hash {} and labeled as '{}'",
                            style(&hash).cyan(),
                            style(label_str).green()
                        );
                    }
                    StoreResponse::Error(e) => {
                        return Err(anyhow::anyhow!("Failed to store content: {}", e));
                    }
                    _ => {
                        return Err(anyhow::anyhow!("Unexpected response from server"));
                    }
                }
            } else {
                // Just store content without a label
                let (tx, rx) = oneshot::channel();

                connection
                    .send(TheaterCommand::StoreOperation {
                        command: StoreCommand::Store { content },
                        response_tx: tx,
                    })
                    .await?;

                match rx.await? {
                    StoreResponse::ContentRef(hash) => {
                        println!("Content stored with hash {}", style(&hash).cyan());
                    }
                    StoreResponse::Error(e) => {
                        return Err(anyhow::anyhow!("Failed to store content: {}", e));
                    }
                    _ => {
                        return Err(anyhow::anyhow!("Unexpected response from server"));
                    }
                }
            }

            Ok(())
        }

        StoreCommands::Get { reference, output } => {
            let content = if reference.starts_with("hash:") {
                // Direct hash reference
                let hash = reference.strip_prefix("hash:").unwrap();
                let (tx, rx) = oneshot::channel();

                connection
                    .send(TheaterCommand::StoreOperation {
                        command: StoreCommand::Get {
                            content_ref: hash.to_string(),
                        },
                        response_tx: tx,
                    })
                    .await?;

                match rx.await? {
                    StoreResponse::Content(content) => content,
                    StoreResponse::Error(e) => {
                        return Err(anyhow::anyhow!("Failed to get content: {}", e));
                    }
                    _ => {
                        return Err(anyhow::anyhow!("Unexpected response from server"));
                    }
                }
            } else {
                // Assume it's a label
                let (tx_label, rx_label) = oneshot::channel();

                connection
                    .send(TheaterCommand::StoreOperation {
                        command: StoreCommand::GetByLabel {
                            label: reference.clone(),
                        },
                        response_tx: tx_label,
                    })
                    .await?;

                let hashes = match rx_label.await? {
                    StoreResponse::ContentRefs(refs) => refs,
                    StoreResponse::Error(e) => {
                        return Err(anyhow::anyhow!("Failed to get content by label: {}", e));
                    }
                    _ => {
                        return Err(anyhow::anyhow!("Unexpected response from server"));
                    }
                };

                match hashes.len() {
                    0 => {
                        return Err(anyhow::anyhow!(
                            "No content found with label: {}",
                            reference
                        ))
                    }
                    1 => {
                        // Get content by hash
                        let (tx, rx) = oneshot::channel();

                        connection
                            .send(TheaterCommand::StoreOperation {
                                command: StoreCommand::Get {
                                    content_ref: hashes[0].to_string(),
                                },
                                response_tx: tx,
                            })
                            .await?;

                        match rx.await? {
                            StoreResponse::Content(content) => content,
                            StoreResponse::Error(e) => {
                                return Err(anyhow::anyhow!("Failed to get content: {}", e));
                            }
                            _ => {
                                return Err(anyhow::anyhow!("Unexpected response from server"));
                            }
                        }
                    }
                    _ => {
                        return Err(anyhow::anyhow!(
                            "Ambiguous label reference, multiple matches found: {}",
                            reference
                        ))
                    }
                }
            };

            if let Some(output_path) = output {
                fs::write(&output_path, &content).await?;
                println!("Content written to {}", style(&output_path).green());
            } else {
                // Try to print as text if possible, otherwise show binary summary
                match std::str::from_utf8(&content) {
                    Ok(text) => {
                        println!("{}", text);
                    }
                    Err(_) => {
                        println!("Binary content ({} bytes)", content.len());
                        println!("Use --output flag to save to a file");
                    }
                }
            }

            Ok(())
        }

        StoreCommands::Label { label, hash } => {
            let (tx, rx) = oneshot::channel();

            connection
                .send(TheaterCommand::StoreOperation {
                    command: StoreCommand::Label {
                        label: label.clone(),
                        content_ref: hash.clone(),
                    },
                    response_tx: tx,
                })
                .await?;

            match rx.await? {
                StoreResponse::Success => {
                    println!(
                        "Content {} labeled as '{}'",
                        style(hash).cyan(),
                        style(label).green()
                    );
                    Ok(())
                }
                StoreResponse::Error(e) => Err(anyhow::anyhow!("Failed to label content: {}", e)),
                _ => Err(anyhow::anyhow!("Unexpected response from server")),
            }
        }

        StoreCommands::ListLabels => {
            let (tx, rx) = oneshot::channel();

            connection
                .send(TheaterCommand::StoreOperation {
                    command: StoreCommand::ListLabels,
                    response_tx: tx,
                })
                .await?;

            match rx.await? {
                StoreResponse::Labels(labels) => {
                    if labels.is_empty() {
                        println!("No labels found in store");
                    } else {
                        println!("{} labels found:", labels.len());
                        for label in labels {
                            println!("- {}", style(&label).green());
                        }
                    }
                    Ok(())
                }
                StoreResponse::Error(e) => Err(anyhow::anyhow!("Failed to list labels: {}", e)),
                _ => Err(anyhow::anyhow!("Unexpected response from server")),
            }
        }

        StoreCommands::ListByLabel { label } => {
            let (tx, rx) = oneshot::channel();

            connection
                .send(TheaterCommand::StoreOperation {
                    command: StoreCommand::GetByLabel {
                        label: label.clone(),
                    },
                    response_tx: tx,
                })
                .await?;

            match rx.await? {
                StoreResponse::ContentRefs(refs) => {
                    if refs.is_empty() {
                        println!("No content found with label '{}'", style(label).green());
                    } else {
                        println!(
                            "{} item(s) found with label '{}':",
                            refs.len(),
                            style(label).green()
                        );
                        for hash in refs {
                            println!("- {}", style(&hash).cyan());
                        }
                    }
                    Ok(())
                }
                StoreResponse::Error(e) => {
                    Err(anyhow::anyhow!("Failed to list content by label: {}", e))
                }
                _ => Err(anyhow::anyhow!("Unexpected response from server")),
            }
        }

        StoreCommands::ListAll => {
            let (tx, rx) = oneshot::channel();

            connection
                .send(TheaterCommand::StoreOperation {
                    command: StoreCommand::ListAllContent,
                    response_tx: tx,
                })
                .await?;

            match rx.await? {
                StoreResponse::ContentRefs(refs) => {
                    if refs.is_empty() {
                        println!("No content found in store");
                    } else {
                        println!("{} item(s) found in store:", refs.len());
                        for hash in refs {
                            println!("- {}", style(&hash).cyan());
                        }
                    }
                    Ok(())
                }
                StoreResponse::Error(e) => {
                    Err(anyhow::anyhow!("Failed to list all content: {}", e))
                }
                _ => Err(anyhow::anyhow!("Unexpected response from server")),
            }
        }

        StoreCommands::Size => {
            let (tx, rx) = oneshot::channel();

            connection
                .send(TheaterCommand::StoreOperation {
                    command: StoreCommand::CalculateTotalSize,
                    response_tx: tx,
                })
                .await?;

            match rx.await? {
                StoreResponse::Size(size) => {
                    let (size_str, unit) = format_bytes(size);
                    println!("Total store size: {} {}", style(size_str).cyan(), unit);
                    Ok(())
                }
                StoreResponse::Error(e) => {
                    Err(anyhow::anyhow!("Failed to calculate store size: {}", e))
                }
                _ => Err(anyhow::anyhow!("Unexpected response from server")),
            }
        }

        StoreCommands::RemoveLabel { label } => {
            let (tx, rx) = oneshot::channel();

            connection
                .send(TheaterCommand::StoreOperation {
                    command: StoreCommand::RemoveLabel {
                        label: label.clone(),
                    },
                    response_tx: tx,
                })
                .await?;

            match rx.await? {
                StoreResponse::Success => {
                    println!("Label '{}' removed", style(label).green());
                    Ok(())
                }
                StoreResponse::Error(e) => Err(anyhow::anyhow!("Failed to remove label: {}", e)),
                _ => Err(anyhow::anyhow!("Unexpected response from server")),
            }
        }

        StoreCommands::RemoveFromLabel { label, hash } => {
            let (tx, rx) = oneshot::channel();

            connection
                .send(TheaterCommand::StoreOperation {
                    command: StoreCommand::RemoveFromLabel {
                        label: label.clone(),
                        content_ref: hash.clone(),
                    },
                    response_tx: tx,
                })
                .await?;

            match rx.await? {
                StoreResponse::Success => {
                    println!(
                        "Content {} removed from label '{}'",
                        style(hash).cyan(),
                        style(label).green()
                    );
                    Ok(())
                }
                StoreResponse::Error(e) => Err(anyhow::anyhow!(
                    "Failed to remove content from label: {}",
                    e
                )),
                _ => Err(anyhow::anyhow!("Unexpected response from server")),
            }
        }
    }
}

fn format_bytes(bytes: u64) -> (String, String) {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        (format!("{:.2}", bytes as f64 / GB as f64), "GB".to_string())
    } else if bytes >= MB {
        (format!("{:.2}", bytes as f64 / MB as f64), "MB".to_string())
    } else if bytes >= KB {
        (format!("{:.2}", bytes as f64 / KB as f64), "KB".to_string())
    } else {
        (bytes.to_string(), "bytes".to_string())
    }
} 
