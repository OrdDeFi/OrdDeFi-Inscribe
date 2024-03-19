use bitcoin::key::{KeyPair, UntweakedKeyPair};
use futures::future::ok;
use super::*;

#[derive(Serialize, Deserialize)]
pub struct Output {
    pub address: Option<String>,
}

#[derive(Debug, Parser)]
pub(crate) struct CommitGenAddr {
    #[arg(
    long,
    default_value = "",
    help = "Use <PRV> to derive private key."
    )]
    pub(crate) prv: String,
    #[arg(
    long,
    default_value = "",
    help = "Use <CONTENT> to derive inscription content."
    )]
    pub(crate) content: String,
}

impl CommitGenAddr {
    fn bytes_from_hex_string(hex_str: &str) -> Result<[u8; 32], &'static str> {
        if hex_str.len() != 64 {
            return Err("Hex string must be exactly 64 characters long");
        }

        match hex::decode(hex_str) {
            Ok(bytes) => {
                if bytes.len() != 32 {
                    Err("Decoded bytes length is not 32")
                } else {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    Ok(arr)
                }
            }
            Err(_) => Err("Failed to decode hex string"),
        }
    }

    fn key_pair_from_str(key: &str) -> Result<KeyPair> {
        let deserialized_key;
        let key_bytes = Self::bytes_from_hex_string(key);
        match key_bytes {
            Ok(bytes) => deserialized_key = bytes,
            Err(e) => bail!("cannot restore private key"),
        }
        let secp256k1_2 = Secp256k1::new();
        let key_pair = bitcoin::key::KeyPair::from_seckey_slice(&secp256k1_2, &deserialized_key).unwrap();
        Ok(key_pair)
    }

    pub(crate) fn run(self) -> SubcommandResult {
        let key_pair = Self::key_pair_from_str(&self.prv).unwrap();

        Ok(Box::new(crate::subcommand::wallet::commit_gen_addr::Output {
            address: None,
        }))
    }
}
