use chrono::{DateTime, Utc};
use skrillax_packet::Packet;
use skrillax_protocol::define_inbound_protocol;
use skrillax_serde::{ByteSize, Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize, ByteSize, Packet, Debug)]
#[packet(opcode = 0x2002)]
pub struct KeepAlive;

#[derive(Clone, Serialize, ByteSize, Deserialize, Packet, Debug)]
#[packet(opcode = 0x2001)]
pub struct IdentityInformation {
    pub module_name: String,
    pub locality: u8,
}

#[derive(Clone, Deserialize, Serialize, ByteSize, Packet, Debug)]
#[packet(opcode = 0x6104)]
pub struct GatewayNoticeRequest {
    pub unknown: u8,
}

#[derive(Clone, Serialize, Deserialize, ByteSize, Packet, Debug)]
#[packet(opcode = 0xA104, massive = true)]
pub struct GatewayNoticeResponse {
    #[silkroad(list_type = "length")]
    pub notices: Vec<GatewayNotice>,
}

#[derive(Clone, Deserialize, Serialize, ByteSize, Debug)]
pub struct GatewayNotice {
    pub subject: String,
    pub article: String,
    pub published: NormalDateTime,
}

type NormalDateTime = DateTime<Utc>;

#[derive(Clone, Serialize, Deserialize, ByteSize, Packet, Debug)]
#[packet(opcode = 0x6100)]
pub struct PatchRequest {
    pub content: u8,
    pub module: String,
    pub version: u32,
}

#[derive(Clone, Serialize, Deserialize, ByteSize, Packet, Debug)]
#[packet(opcode = 0xA100, massive = true)]
pub struct PatchResponse {
    pub result: PatchResult,
}

#[derive(Clone, Deserialize, Serialize, ByteSize, Debug)]
pub enum PatchResult {
    #[silkroad(value = 1)]
    UpToDate { unknown: u8 },
    #[silkroad(value = 2)]
    Problem { error: PatchError },
}

#[derive(Clone, Deserialize, Serialize, ByteSize, Debug)]
pub enum PatchError {
    #[silkroad(value = 1)]
    InvalidVersion,
    #[silkroad(value = 2)]
    Update {
        server_ip: String,
        server_port: u16,
        current_version: u32,
        #[silkroad(list_type = "has-more")]
        patch_files: Vec<PatchFile>,
        http_server: String,
    },
    #[silkroad(value = 3)]
    Offline,
    #[silkroad(value = 4)]
    InvalidClient,
    #[silkroad(value = 5)]
    PatchDisabled,
}

#[derive(Clone, Deserialize, Serialize, ByteSize, Debug)]
pub struct PatchFile {
    pub file_id: u32,
    pub filename: String,
    pub file_path: String,
    pub size: u32,
    pub in_pk2: bool,
}

define_inbound_protocol! { PatchProtocol =>
    KeepAlive,
    PatchRequest,
    IdentityInformation,
    GatewayNoticeRequest
}
