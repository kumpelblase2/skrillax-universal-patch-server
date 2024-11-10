mod protocol;

use crate::protocol::{
    GatewayNoticeResponse, IdentityInformation, PatchError, PatchProtocol, PatchResponse,
    PatchResult,
};
use skrillax_stream::handshake::{ActiveSecuritySetup, PassiveSecuritySetup};
use skrillax_stream::stream::SilkroadTcpExt;
use std::collections::HashSet;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::net::{TcpSocket, TcpStream};
use tokio::signal;
use tokio_util::sync::CancellationToken;
use walkdir::WalkDir;

#[derive(Clone)]
struct PatchFileserver {
    ip: String,
    host: String,
    base_path: String,
}

impl PatchFileserver {
    pub fn ip(&self) -> &str {
        &self.ip
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn base_path(&self) -> &str {
        &self.base_path
    }
}

struct Patch {
    version: u16,
    files: Box<[PathBuf]>,
}

struct PatchProvider {
    patches: RwLock<Vec<Patch>>, // lets assume/ensure this is sorted according to the patch version ascending
    patch_dir: PathBuf,
    server: PatchFileserver,
}

struct PatchFile {
    file: PathBuf,
    patch: u16,
}

impl PatchProvider {
    pub fn new(patch_dir: PathBuf, fileserver: PatchFileserver) -> PatchProvider {
        PatchProvider {
            patch_dir,
            patches: RwLock::new(Vec::new()),
            server: fileserver,
        }
    }

    pub fn fileserver(&self) -> &PatchFileserver {
        &self.server
    }

    pub fn add_patch(&self, version: u16, files: Box<[PathBuf]>) {
        self.patches.write().unwrap().push(Patch { version, files })
    }

    pub fn patch_dir(&self) -> &Path {
        &self.patch_dir
    }

    pub fn collect_necessary_files(&self, current: u16, target: u16) -> Vec<PatchFile> {
        let patches = self.patches.read().unwrap();
        if current > target {
            let files_to_revert = patches
                .iter()
                .filter(|patch| patch.version > target && patch.version <= current)
                .flat_map(|patch| patch.files.iter().cloned())
                .collect::<HashSet<PathBuf>>();

            files_to_revert
                .into_iter()
                .filter_map(|file| {
                    get_latest_version_in_up_to(&file, &patches, target).map(|version| PatchFile {
                        file,
                        patch: version,
                    })
                })
                .collect()
        } else {
            let applicable_versions = patches
                .iter()
                .filter(|patch| patch.version > current && patch.version <= target)
                .collect::<Vec<&Patch>>();

            // we need to track which files have been updated in which version (and which latest version of it)
            let all_files = applicable_versions
                .iter()
                .flat_map(|patch| patch.files.iter().cloned())
                .collect::<HashSet<PathBuf>>();

            all_files
                .into_iter()
                .filter_map(|file| {
                    get_latest_version_in(&file, &applicable_versions).map(|version| PatchFile {
                        file,
                        patch: version,
                    })
                })
                .collect()
        }
    }
}

fn get_latest_version_in(file: &Path, patches: &[&Patch]) -> Option<u16> {
    for patch in patches.iter().rev() {
        if patch.files.iter().any(|f| f == file) {
            return Some(patch.version);
        }
    }

    None
}

fn get_latest_version_in_up_to(file: &Path, patches: &[Patch], min_version: u16) -> Option<u16> {
    for patch in patches.iter().rev() {
        if patch.version <= min_version && patch.files.iter().any(|f| f.as_path() == file) {
            return Some(patch.version);
        }
    }

    None
}

struct SocketCoordinator {
    patch_provider: Arc<PatchProvider>,
    cancel_token: CancellationToken,
}

impl SocketCoordinator {
    pub fn new(patch_provider: Arc<PatchProvider>) -> SocketCoordinator {
        SocketCoordinator {
            patch_provider,
            cancel_token: CancellationToken::new(),
        }
    }

    pub fn accept_patch(&mut self, patch: u16) {
        let result = TcpSocket::new_v4().unwrap();
        let port = 32000 + patch;
        result
            .bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port))
            .unwrap();
        let provider = Arc::clone(&self.patch_provider);
        let cancel_token = self.cancel_token.clone();
        tokio::spawn(async move {
            let listener = result.listen(5).unwrap();

            // TODO: try to recreate the socket on error
            while let Some(Ok(accepted)) = tokio::select! {
                res = listener.accept() => Some(res),
                _ = cancel_token.cancelled() => None,
            } {
                let (stream, _) = accepted;
                let patch_provider = Arc::clone(&provider);
                let child_token = cancel_token.child_token();
                tokio::spawn(async move {
                    handle_client(stream, patch, patch_provider, child_token).await;
                });
            }
        });
    }

    pub fn shutdown(&self) {
        self.cancel_token.cancel()
    }
}

async fn handle_client(
    client: TcpStream,
    target_version: u16,
    patch_provider: Arc<PatchProvider>,
    child_token: CancellationToken,
) {
    let (mut reader, mut writer) = client.into_silkroad_stream();
    ActiveSecuritySetup::handle(&mut reader, &mut writer)
        .await
        .unwrap();

    while let Some(Ok(packet)) = tokio::select! {
        p = reader.next_packet::<PatchProtocol>() => Some(p),
        _ = child_token.cancelled() => None
    } {
        match *packet {
            PatchProtocol::KeepAlive(_) => {}
            PatchProtocol::PatchRequest(request) => {
                let current_version = request.version;
                let result = if current_version == target_version.into() {
                    PatchResult::UpToDate { unknown: 0 }
                } else {
                    let patches = patch_provider
                        .collect_necessary_files(current_version as u16, target_version);

                    let fileserver = patch_provider.fileserver();

                    PatchResult::Problem {
                        error: PatchError::Update {
                            server_ip: fileserver.ip().to_string(),
                            server_port: 80,
                            current_version: target_version.into(),
                            patch_files: patches
                                .into_iter()
                                .enumerate()
                                .map(|(index, file)| {
                                    let in_pk2 = file.file.parent().is_some();
                                    let filename = PathBuf::from(&file.file);
                                    let filename =
                                        filename.file_name().unwrap().to_str().unwrap().to_string();
                                    let size = get_filesize_of(patch_provider.patch_dir(), &file);
                                    protocol::PatchFile {
                                        file_id: index as u32,
                                        filename,
                                        file_path: format!(
                                            "{}/{}/{}",
                                            fileserver.base_path(),
                                            file.patch,
                                            file.file.to_str().unwrap()
                                        ),
                                        size,
                                        in_pk2,
                                    }
                                })
                                .collect(),
                            http_server: fileserver.host().to_string(),
                        },
                    }
                };

                writer.write_packet(PatchResponse { result }).await.unwrap()
            }
            PatchProtocol::IdentityInformation(_) => writer
                .write_packet(IdentityInformation {
                    module_name: "GatewayServer".to_string(),
                    locality: 0x12,
                })
                .await
                .unwrap(),
            PatchProtocol::GatewayNoticeRequest(_) => {
                writer
                    .write_packet(GatewayNoticeResponse { notices: vec![] })
                    .await
                    .unwrap();
            }
        }
    }
}

fn get_filesize_of(patch_dir: &Path, file: &PatchFile) -> u32 {
    let absolute_file = patch_dir.join(file.patch.to_string()).join(&file.file);
    fs::metadata(absolute_file).unwrap().len() as u32
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let local_patch_dir = "./patches";
    let local_patch_dir = PathBuf::from(local_patch_dir);
    let patches = load_patches(&local_patch_dir);
    let patch_versions = patches.iter().map(|p| p.version).collect::<Vec<u16>>();
    let patch_provider = PatchProvider::new(
        local_patch_dir,
        PatchFileserver {
            ip: "127.0.0.1".to_string(),
            host: "localhost".to_string(),
            base_path: "".to_string(),
        },
    );
    for patch in patches {
        patch_provider.add_patch(patch.version, patch.files);
    }
    let patch_provider = Arc::new(patch_provider);
    let mut coordinator = SocketCoordinator::new(patch_provider);
    for patch in patch_versions {
        coordinator.accept_patch(patch);
    }

    signal::ctrl_c()
        .await
        .expect("Should be able to listen for ctrl-c");

    coordinator.shutdown();
}

fn load_patches(local_path: &Path) -> Vec<Patch> {
    local_path
        .read_dir()
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| {
            let patch: u16 = entry.file_name().into_string().unwrap().parse().unwrap();
            let patch_files = collect_files_recursively(&entry.path());

            Patch {
                version: patch,
                files: patch_files.into_boxed_slice(),
            }
        })
        .collect()
}

fn collect_files_recursively(path: &Path) -> Vec<PathBuf> {
    WalkDir::new(path)
        .same_file_system(true)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|dir| dir.metadata().unwrap().is_file())
        .map(|file| file.path().strip_prefix(path).unwrap().to_path_buf())
        .collect()
}
