use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, DnValue::PrintableString,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    server::ClientHello,
};
use std::collections::HashMap;
use std::path::Path;
use std::{fs, sync::Arc};
use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;

const DEFAULT_VALIDITY_DAYS: u32 = 3650;

#[derive(Clone)]
pub struct TlsConfig {
    ca: Arc<CaConfig>,
    server_configs: Arc<RwLock<HashMap<String, Arc<ServerConfig>>>>,
}

impl TlsConfig {
    pub fn new(ca_path: &Path) -> Self {
        Self {
            ca: Arc::new(CaConfig::load_or_gen(ca_path)),
            server_configs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn server_config(&self, hello: ClientHello<'_>) -> Arc<ServerConfig> {
        let server_name = hello.server_name().unwrap_or("");
        if let Some(config) = self.server_configs.read().await.get(server_name).cloned() {
            config
        } else {
            let new_config = self.config_for_server(server_name);
            let mut configs = self.server_configs.write().await;

            // Double check config doesn't exist to avoid a race.
            if let Some(config) = configs.get(server_name) {
                return config.clone();
            } else {
                configs.insert(server_name.to_string(), Arc::new(new_config));
                return configs.get(server_name).unwrap().clone();
            }
        }
    }

    fn cert_for_domains(&self, domains: Vec<String>) -> (String, String) {
        let (cert, key) = gen_cert(&self.ca.cert, &self.ca.key, domains);
        (cert.pem(), key.serialize_pem())
    }

    fn config_for_server(&self, server_name: &str) -> ServerConfig {
        let (cert_pem, key_pem) = self.cert_for_domains(vec![server_name.to_string()]);
        let cert = CertificateDer::from_pem_slice(cert_pem.as_bytes()).unwrap();
        let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes()).unwrap();

        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .unwrap();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec(), b"http/1.0".to_vec()];
        config
    }
}

struct CaConfig {
    cert: Certificate,
    key: KeyPair,
}

impl CaConfig {
    fn load_or_gen(path: &Path) -> Self {
        let mut key_path = path.to_path_buf();
        let mut cert_path = path.to_path_buf();
        key_path.push("ca_key.pem");
        cert_path.push("ca_cert.pem");
        let key = if let Ok(key) = fs::read_to_string(&key_path) {
            let key = KeyPair::from_pem(&key).unwrap();
            if let Ok(cert) = fs::read_to_string(&cert_path) {
                // TODO check if certificate is still valid, otherwise regenerate.
                let ca_params = CertificateParams::from_ca_cert_pem(&cert).unwrap();
                let cert = ca_params.self_signed(&key).unwrap();
                return Self { key, cert };
            }
            key
        } else {
            let key = KeyPair::generate().unwrap();
            fs::write(&key_path, key.serialize_pem()).unwrap();
            key
        };
        let cert = gen_ca(&key);
        fs::write(&cert_path, cert.pem()).unwrap();

        Self { key, cert }
    }
}

fn gen_ca(key_pair: &KeyPair) -> Certificate {
    let mut params = CertificateParams::new(Vec::default()).unwrap();
    let (not_before, not_after) = validity_period();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.distinguished_name.push(
        DnType::CountryName,
        PrintableString("US".try_into().unwrap()),
    );
    params
        .distinguished_name
        .push(DnType::OrganizationName, "Wirejack Proxy CA");
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);
    params.key_usages.push(KeyUsagePurpose::KeyCertSign);
    params.key_usages.push(KeyUsagePurpose::CrlSign);

    params.not_before = not_before;
    params.not_after = not_after;

    params.self_signed(key_pair).unwrap()
}

fn gen_cert(ca: &Certificate, ca_key: &KeyPair, domains: Vec<String>) -> (Certificate, KeyPair) {
    let mut params = CertificateParams::new(domains).expect("Invalid domain names");
    let (not_before, not_after) = validity_period();
    params
        .distinguished_name
        .push(DnType::CommonName, "Wirejack Proxy");
    params.use_authority_key_identifier_extension = true;
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);
    params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);
    params.not_before = not_before;
    params.not_after = not_after;

    let key_pair = KeyPair::generate().unwrap();
    (params.signed_by(&key_pair, ca, ca_key).unwrap(), key_pair)
}

fn validity_period() -> (OffsetDateTime, OffsetDateTime) {
    let day = Duration::new(86400, 0);
    let expiry = day * DEFAULT_VALIDITY_DAYS;
    let not_before = OffsetDateTime::now_utc().checked_sub(day).unwrap();
    let not_after = OffsetDateTime::now_utc().checked_add(expiry).unwrap();
    (not_before, not_after)
}
