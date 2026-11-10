//! 应用通道客户端（自 `kit/channel/client` 搬入）。
//!
//! 提供 `ChannelClient` 封装，建立到服务端的 gRPC 双向长连接。
//!
//! 协议契约以 `kit/channel/proto/proto/channel.proto` 单份契约为准，
//! 本 crate 内 `proto/channel.proto` 为其副本、`src/channel.v1.rs` 为
//! tonic-prost-build 0.14 生成的 client-only 代码（已提交，构建无需 protoc）。
//! 协议变更时：同步 proto 副本 → 重新生成并替换 `channel.v1.rs`。

/// protobuf 包 `channel.v1` 生成的 client 代码。
pub mod channel {
    include!("channel.v1.rs");
}

pub mod client;

pub use client::{ChannelClient, ChannelClientConfig, ChannelError};

/// 当前协议包名，用于日志和协议检查。
pub const PROTOCOL_PACKAGE: &str = "channel.v1";

/// 终端双向流 RPC 全路径。
pub const CHANNEL_PATH: &str = "/channel.v1.ChannelService/Open";

#[cfg(test)]
mod tests {
    use crate::channel::{
        ClientMessage, DeviceInfo, Hello, PushMessage, ServerMessage, TaskAck, TaskAssign,
        TaskProgress, TaskStatus, TaskType, UserInfo, client_message::Payload as ClientPayload,
        server_message::Payload as ServerPayload,
    };
    use prost::Message;

    #[test]
    fn hello_message_roundtrip() {
        let message = ClientMessage {
            payload: Some(ClientPayload::Hello(Hello {
                device_id: "device-1".into(),
                app_key: "demo-app".into(),
                version: "0.1.0".into(),
                device_info: Some(DeviceInfo {
                    device_brand: "Apple".into(),
                    device_model: "Mac16,8".into(),
                    device_type: "laptop".into(),
                    screen_width: 2560,
                    screen_height: 1440,
                    os_name: "macos".into(),
                    os_version: "27.0.1".into(),
                    wifi_enabled: true,
                    bluetooth_enabled: false,
                    host_version: "0.1.5".into(),
                }),
                // 登录态用户资料: 未登录不填(与 kit 服务端 Hello 语义一致)
                user_info: Some(UserInfo {
                    user_id: "user-1".into(),
                    username: "user-1".into(),
                    nickname: "用户一".into(),
                    mobile: "13800138000".into(),
                    avatar: "https://example.com/a.png".into(),
                }),
            })),
        };

        let encoded = message.encode_to_vec();
        let decoded = ClientMessage::decode(encoded.as_slice()).expect("decode hello message");

        match decoded.payload {
            Some(ClientPayload::Hello(hello)) => {
                assert_eq!(hello.device_id, "device-1");
                assert_eq!(hello.app_key, "demo-app");
                let info = hello.device_info.expect("device_info present");
                assert_eq!(info.device_model, "Mac16,8");
                assert_eq!(info.screen_width, 2560);
                assert!(info.wifi_enabled);
                let user = hello.user_info.expect("user_info present");
                assert_eq!(user.user_id, "user-1");
                assert_eq!(user.nickname, "用户一");
                assert_eq!(user.mobile, "13800138000");
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn user_info_frame_roundtrip() {
        // 上行补报帧(登录后/变更时)
        let report = ClientMessage {
            payload: Some(ClientPayload::UserInfo(UserInfo {
                user_id: "user-1".into(),
                username: "user-1".into(),
                nickname: "用户一".into(),
                mobile: "13800138000".into(),
                avatar: String::new(),
            })),
        };
        let decoded =
            ClientMessage::decode(report.encode_to_vec().as_slice()).expect("decode user info");

        match decoded.payload {
            Some(ClientPayload::UserInfo(user)) => {
                assert_eq!(user.user_id, "user-1");
                assert_eq!(user.nickname, "用户一");
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        // 解绑帧: user_id 空 = 登出解绑(契约要求的显式解绑语义)
        let unbind = ClientMessage {
            payload: Some(ClientPayload::UserInfo(UserInfo::default())),
        };
        let decoded =
            ClientMessage::decode(unbind.encode_to_vec().as_slice()).expect("decode unbind");

        match decoded.payload {
            Some(ClientPayload::UserInfo(user)) => {
                assert!(user.user_id.is_empty());
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn task_assign_and_ack_roundtrip() {
        let assign = ServerMessage {
            payload: Some(ServerPayload::TaskAssign(TaskAssign {
                task_id: 1001,
                task_type: TaskType::Upgrade as i32,
                attempt: 1,
                payload: r#"{"version":"0.2.0"}"#.into(),
            })),
        };
        let decoded =
            ServerMessage::decode(assign.encode_to_vec().as_slice()).expect("decode task assign");
        match decoded.payload {
            Some(ServerPayload::TaskAssign(task)) => {
                assert_eq!(task.task_id, 1001);
                assert_eq!(TaskType::try_from(task.task_type), Ok(TaskType::Upgrade));
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        let ack = ClientMessage {
            payload: Some(ClientPayload::TaskAck(TaskAck {
                task_id: 1001,
                attempt: 1,
                success: true,
                message: String::new(),
            })),
        };
        let decoded =
            ClientMessage::decode(ack.encode_to_vec().as_slice()).expect("decode task ack");
        match decoded.payload {
            Some(ClientPayload::TaskAck(ack)) => {
                assert_eq!(ack.task_id, 1001);
                assert!(ack.success);
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn task_progress_roundtrip() {
        let progress = ClientMessage {
            payload: Some(ClientPayload::TaskProgress(TaskProgress {
                task_id: 1001,
                status: TaskStatus::Running as i32,
                result: "downloading".into(),
            })),
        };
        let decoded =
            ClientMessage::decode(progress.encode_to_vec().as_slice()).expect("decode progress");
        match decoded.payload {
            Some(ClientPayload::TaskProgress(p)) => {
                assert_eq!(p.task_id, 1001);
                assert_eq!(TaskStatus::try_from(p.status), Ok(TaskStatus::Running));
                assert_eq!(p.result, "downloading");
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn push_message_roundtrip() {
        let message = ServerMessage {
            payload: Some(ServerPayload::Push(PushMessage {
                message_id: "push-1".into(),
                message_type: "notify".into(),
                payload: r#"{"text":"hello"}"#.into(),
                server_time: 1_700_000_000,
            })),
        };

        let decoded =
            ServerMessage::decode(message.encode_to_vec().as_slice()).expect("decode push message");
        match decoded.payload {
            Some(ServerPayload::Push(push)) => {
                assert_eq!(push.message_id, "push-1");
                assert_eq!(push.message_type, "notify");
                assert_eq!(push.payload, r#"{"text":"hello"}"#);
                assert_eq!(push.server_time, 1_700_000_000);
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }
}
