


use cloudevents::{event::AttributeValue, Data, Event};
use drogue_ajour_protocol::{Status};

use futures::{stream::StreamExt, TryFutureExt};
use paho_mqtt as mqtt;



use crate::updater::Updater;

pub struct Server {
    client: mqtt::AsyncClient,
    application: String,
    updater: Updater,
}

impl Server {
    pub fn new(client: mqtt::AsyncClient, application: String, updater: Updater) -> Self {
        Self {
            client,
            application,
            updater,
        }
    }

    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        let mut stream = self.client.get_stream(100);
        self.client
            .subscribe(format!("app/{}", &self.application), 1);
        loop {
            if let Some(m) = stream.next().await {
                if let Some(m) = m {
                    match serde_json::from_slice::<Event>(m.payload()) {
                        Ok(e) => {
                            let mut is_dfu = false;
                            let mut application = String::new();
                            let mut device = String::new();
                            for a in e.iter() {
                                log::trace!("Attribute {:?}", a);
                                if a.0 == "subject" {
                                    if let AttributeValue::String("dfu") = a.1 {
                                        is_dfu = true;
                                    }
                                } else if a.0 == "device" {
                                    if let AttributeValue::String(d) = a.1 {
                                        device = d.to_string();
                                    }
                                } else if a.0 == "application" {
                                    if let AttributeValue::String(d) = a.1 {
                                        application = d.to_string();
                                    }
                                }
                            }

                            log::trace!(
                                "Event from app {}, device {}, is dfu: {}",
                                application,
                                device,
                                is_dfu
                            );

                            if is_dfu {
                                let status: Option<Result<Status, anyhow::Error>> =
                                    e.data().map(|d| match d {
                                        Data::Binary(b) => {
                                            serde_cbor::from_slice(&b[..]).map_err(|e| e.into())
                                        }
                                        Data::String(s) => {
                                            serde_json::from_str(&s).map_err(|e| e.into())
                                        }
                                        Data::Json(v) => serde_json::from_str(v.as_str().unwrap())
                                            .map_err(|e| e.into()),
                                    });

                                log::trace!("Status decode: {:?}", status);

                                if let Some(Ok(status)) = status {
                                    log::info!("Received status from {}: {:?}", device, status);
                                    if let Ok(command) =
                                        self.updater.process(&application, &device, status).await
                                    {
                                        log::info!("Sending command to {}: {:?}", device, command);

                                        let topic =
                                            format!("command/{}/{}/dfu", application, device);
                                        let message = mqtt::Message::new(
                                            topic,
                                            serde_cbor::to_vec(&command)?,
                                            1,
                                        );
                                        if let Err(e) = self.client.publish(message).await {
                                            log::info!(
                                                "Error publishing command back to device: {:?}",
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("Error parsing event: {:?}", e);
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
