use crate::proto::messages::horust_msg_message::MessageType::Request;
use crate::proto::messages::{horust_msg_message, horust_msg_request, horust_msg_response, HorustChangeServiceStatus, HorustMsgError, HorustMsgMessage, HorustMsgRequest, HorustMsgResponse, HorustMsgServiceChangeResponse, HorustMsgServiceInfoResponse, HorustMsgServiceStatus, HorustMsgServiceStatusResponse};
use crate::UdsConnectionHandler;
use anyhow::{anyhow, Result};
use log::{error, info};
use std::io::ErrorKind;
use std::os::unix::net::UnixListener;

pub trait CommandsHandlerTrait {
    fn start(&mut self) -> Result<()> {
        // put the server logic in a loop to accept several connections
        loop {
            self.accept().expect("TODO: panic message");
        }
    }
    fn get_unix_listener(&mut self) -> &mut UnixListener;
    fn accept(&mut self) -> Result<()> {
        match self.get_unix_listener().accept() {
            Ok((stream, _addr)) => {
                let conn_handler = UdsConnectionHandler::new(stream);
                if let Err(err) = self.handle_connection(conn_handler) {
                    //todo: send response back.
                    error!("Error handling connection: {}", err);
                }
            }
            Err(e) => {
                let kind = e.kind();
                if !matches!(kind, ErrorKind::WouldBlock) {
                    error!("Error accepting connection: {e} - you might need to restart Horust.");
                }
            }
        };
        Ok(())
    }
    fn handle_connection(&self, mut uds_conn_handler: UdsConnectionHandler) -> Result<()> {
        let received = uds_conn_handler
            .receive_message()?
            .message_type
            .ok_or(anyhow!("No request found in message sent from client."))?;

        if let Request(HorustMsgRequest {
            request: Some(request),
        }) = received
        {
            let response = match request {
                horust_msg_request::Request::StatusRequest(status_request) => {
                    info!("Requested status for {}", status_request.service_name);

                    let service_status = self.get_service_status(&status_request.service_name);
                    service_status
                        .map(|status| {
                            new_horust_msg_service_status_response(
                                status_request.service_name,
                                status
                            )
                        })
                        .unwrap_or_else(|err| {
                            new_horust_msg_error_response(format!(
                                "Error from status handler: {err}",
                            ))
                        })
                }
                horust_msg_request::Request::InfoRequest(info_request) => {
                    info!("Requested info for {}", info_request.service_name);

                    let service_info = self.get_service_info(&info_request.service_name);
                    service_info
                        .map(|info| {
                            new_horust_msg_service_info_response(
                                info_request.service_name,
                                info
                            )
                        })
                        .unwrap_or_else(|err| {
                            new_horust_msg_error_response(format!(
                                "Error from status handler: {err}",
                            ))
                        })
                }
                horust_msg_request::Request::ChangeRequest(change_request) => {
                    info!(
                        "Requested service update for {} to {}",
                        change_request.service_name, change_request.service_status
                    );

                    self.update_service_status(
                        &change_request.service_name,
                        HorustChangeServiceStatus::try_from(change_request.service_status).unwrap(),
                    )
                    .map(|new_status| {
                        new_horust_msg_service_change_response(
                            change_request.service_name,
                            new_status,
                        )
                    })
                    .unwrap_or_else(|err| {
                        new_horust_msg_error_response(format!("Error from change handler: {err}"))
                    })
                }
            };
            uds_conn_handler.send_message(response)?;
        }
        Ok(())
    }

    fn get_service_status(&self, service_name: &str) -> Result<HorustMsgServiceStatus>;

    fn get_service_info(&self, service_name: &str) -> Result<String>;

    fn update_service_status(
        &self,
        service_name: &str,
        new_status: HorustChangeServiceStatus,
    ) -> Result<HorustMsgServiceStatus>;
}

pub fn new_horust_msg_error_response(error: String) -> HorustMsgMessage {
    HorustMsgMessage {
        message_type: Some(horust_msg_message::MessageType::Response(
            HorustMsgResponse {
                response: Some(horust_msg_response::Response::Error(HorustMsgError {
                    error_string: error,
                })),
            },
        )),
    }
}

pub fn new_horust_msg_service_status_response(
    service_name: String,
    status: HorustMsgServiceStatus,
) -> HorustMsgMessage {
    HorustMsgMessage {
        message_type: Some(horust_msg_message::MessageType::Response(
            HorustMsgResponse {
                response: Some(horust_msg_response::Response::StatusResponse(
                    HorustMsgServiceStatusResponse {
                        service_name,
                        service_status: status.into(),
                    },
                )),
            },
        )),
    }
}

pub fn new_horust_msg_service_change_response(
    service_name: String,
    status: HorustMsgServiceStatus,
) -> HorustMsgMessage {
    HorustMsgMessage {
        message_type: Some(horust_msg_message::MessageType::Response(
            HorustMsgResponse {
                response: Some(horust_msg_response::Response::ChangeResponse(
                    HorustMsgServiceChangeResponse {
                        service_name,
                        service_status: status.into(),
                    },
                )),
            },
        )),
    }
}

pub fn new_horust_msg_service_info_response(
    service_name: String,
    info:String
) -> HorustMsgMessage {
    HorustMsgMessage {
        message_type: Some(horust_msg_message::MessageType::Response(
            HorustMsgResponse {
                response: Some(horust_msg_response::Response::InfoResponse(
                    HorustMsgServiceInfoResponse {
                        service_name,
                        info: info
                    },
                )),
            },
        )),
    }
}
