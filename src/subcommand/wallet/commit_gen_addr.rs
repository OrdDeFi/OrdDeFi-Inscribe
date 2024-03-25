use bitcoin::key::{KeyPair, UntweakedKeyPair, XOnlyPublicKey};
use bitcoin::taproot::{LeafVersion, TaprootBuilder};
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
    #[arg(long, help = "Inscribe sat with contents of <FILE>.")]
    pub(crate) file: Option<PathBuf>,
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

    fn key_pair_from_str(key: &str) -> Result<(Secp256k1<All> , KeyPair)> {
        let deserialized_key;
        let key_bytes = Self::bytes_from_hex_string(key);
        match key_bytes {
            Ok(bytes) => deserialized_key = bytes,
            Err(e) => bail!("cannot restore private key"),
        }
        let secp256k1 = Secp256k1::new();
        let key_pair = bitcoin::key::KeyPair::from_seckey_slice(&secp256k1, &deserialized_key).unwrap();
        Ok((secp256k1, key_pair))
    }

    pub(crate) fn run(self, options: Options) -> SubcommandResult {
        let chain = options.chain();

        let inscriptions = if let Some(file_path) = &self.file {
            vec![Inscription::from_file(
                chain,
                file_path,
                None,
                None,
                None,
                None,
                false,
            )?]
        } else {
            // Handle the case when self.file is None
            // For example, return an empty vector or do something else
            vec![]
        };

        let (secp256k1, key_pair) = Self::key_pair_from_str(&self.prv).unwrap();
        let (public_key, _parity) = XOnlyPublicKey::from_keypair(&key_pair);

        let reveal_script = Inscription::append_batch_reveal_script(
            &inscriptions,
            ScriptBuf::builder()
                .push_slice(public_key.serialize())
                .push_opcode(opcodes::all::OP_CHECKSIG),
        );
        // println!("reveal_script {:?}", reveal_script);

        let taproot_spend_info = TaprootBuilder::new()
            .add_leaf(0, reveal_script.clone())
            .expect("adding leaf should work")
            .finalize(&secp256k1, public_key)
            .expect("finalizing taproot builder should work");
        // println!("taproot_spend_info {:?}", taproot_spend_info);

        // let control_block = taproot_spend_info
        //     .control_block(&(reveal_script.clone(), LeafVersion::TapScript))
        //     .expect("should compute control block");
        // println!("control_block {:?}", control_block);

        let commit_tx_address = Address::p2tr_tweaked(taproot_spend_info.output_key(), chain.network());
        // println!("commit_tx_address(temp addr) {:?}", commit_tx_address);

        Ok(Box::new(crate::subcommand::wallet::commit_gen_addr::Output {
            address: Option::from(commit_tx_address.to_string()),
        }))
    }
}
