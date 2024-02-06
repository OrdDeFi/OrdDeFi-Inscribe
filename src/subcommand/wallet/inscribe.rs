use {
  self::batch::{Batch, Batchfile, Mode},
  super::*,
  crate::subcommand::wallet::transaction_builder::Target,
  bitcoin::{
    blockdata::{opcodes, script},
    key::PrivateKey,
    key::{TapTweak, TweakedKeyPair, TweakedPublicKey, UntweakedKeyPair},
    policy::MAX_STANDARD_TX_WEIGHT,
    secp256k1::{self, constants::SCHNORR_SIGNATURE_SIZE, rand, Secp256k1, XOnlyPublicKey},
    sighash::{Prevouts, SighashCache, TapSighashType},
    taproot::Signature,
    taproot::{ControlBlock, LeafVersion, TapLeafHash, TaprootBuilder},
  },
  bitcoincore_rpc::bitcoincore_rpc_json::{ImportDescriptors, SignRawTransactionInput, Timestamp},
  bitcoincore_rpc::Client,
};

mod batch;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct InscriptionInfo {
  pub id: InscriptionId,
  pub location: SatPoint,
}

#[derive(Serialize, Deserialize)]
pub struct Output {
  pub commit: Option<Txid>,
  pub inscriptions: Vec<InscriptionInfo>,
  pub parent: Option<InscriptionId>,
  pub reveal: Txid,
  pub total_fees: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct ParentInfo {
  destination: Address,
  id: InscriptionId,
  location: SatPoint,
  tx_out: TxOut,
}

#[derive(Debug, Parser)]
#[clap(
  group = ArgGroup::new("source")
      .required(true)
      .args(&["file", "batch"]),
)]
pub(crate) struct Inscribe {
  #[arg(
    long,
    help = "Inscribe multiple inscriptions defined in a yaml <BATCH_FILE>.",
    conflicts_with_all = &[
      "cbor_metadata", "destination", "file", "json_metadata", "metaprotocol", "parent", "postage", "reinscribe", "satpoint"
    ]
  )]
  pub(crate) batch: Option<PathBuf>,
  #[arg(
    long,
    help = "Include CBOR in file at <METADATA> as inscription metadata",
    conflicts_with = "json_metadata"
  )]
  pub(crate) cbor_metadata: Option<PathBuf>,
  #[arg(
    long,
    help = "Use <COMMIT_FEE_RATE> sats/vbyte for commit transaction.\nDefaults to <FEE_RATE> if unset."
  )]
  pub(crate) commit_fee_rate: Option<FeeRate>,
  #[arg(long, help = "Compress inscription content with brotli.")]
  pub(crate) compress: bool,
  #[arg(long, help = "Send instruction to <DESTINATION>.")]
  pub(crate) destination: Option<Address<NetworkUnchecked>>,
  #[arg(long, help = "Send change to <CHANGE>.")]
  pub(crate) change: Option<Address<NetworkUnchecked>>,
  #[arg(long, help = "Don't sign or broadcast transactions.")]
  pub(crate) dry_run: bool,
  #[arg(long, help = "Use fee rate of <FEE_RATE> sats/vB.")]
  pub(crate) fee_rate: FeeRate,
  #[arg(long, help = "Inscribe sat with contents of <FILE>.")]
  pub(crate) file: Option<PathBuf>,
  #[arg(
    long,
    help = "Include JSON in file at <METADATA> converted to CBOR as inscription metadata",
    conflicts_with = "cbor_metadata"
  )]
  pub(crate) json_metadata: Option<PathBuf>,
  #[clap(long, help = "Set inscription metaprotocol to <METAPROTOCOL>.")]
  pub(crate) metaprotocol: Option<String>,
  #[arg(long, help = "Do not back up recovery key.")]
  pub(crate) no_backup: bool,
  #[arg(
    long,
    help = "Do not check that transactions are equal to or below the MAX_STANDARD_TX_WEIGHT of 400,000 weight units. Transactions over this limit are currently nonstandard and will not be relayed by bitcoind in its default configuration. Do not use this flag unless you understand the implications."
  )]
  pub(crate) no_limit: bool,
  #[clap(long, help = "Make inscription a child of <PARENT>.")]
  pub(crate) parent: Option<InscriptionId>,
  #[arg(
    long,
    help = "Amount of postage to include in the inscription. Default `10000sat`."
  )]
  pub(crate) postage: Option<Amount>,
  #[clap(long, help = "Allow reinscription.")]
  pub(crate) reinscribe: bool,
  #[arg(long, help = "Inscribe <SATPOINT>.")]
  pub(crate) satpoint: Option<SatPoint>,
  #[arg(long, help = "Inscribe <SAT>.", conflicts_with = "satpoint")]
  pub(crate) sat: Option<Sat>,
}

impl Inscribe {
  pub(crate) fn run(self, wallet: String, options: Options) -> SubcommandResult {
    let metadata = Inscribe::parse_metadata(self.cbor_metadata, self.json_metadata)?;

    let index = Index::open(&options)?;
    // index.update()?;

    let client = bitcoin_rpc_client_for_wallet_command(wallet, &options)?;

    let utxos = get_unspent_outputs(&client, &index)?;

    let locked_utxos = get_locked_outputs(&client)?;

    let runic_utxos = BTreeSet::new();

    let chain = options.chain();

    let postage;
    let destinations;
    let changes;
    let inscriptions;
    let mode;
    let parent_info;
    let sat;

    changes = vec![match self.change.clone() {
      Some(change) => change.require_network(chain.network())?,
      None => get_change_address(&client, chain)?,
    }];

    match (self.file, self.batch) {
      (Some(file), None) => {
        parent_info = None;

        postage = self.postage.unwrap_or(TARGET_POSTAGE);

        inscriptions = vec![Inscription::from_file(
          chain,
          file,
          self.parent,
          None,
          self.metaprotocol,
          metadata,
          self.compress,
        )?];

        mode = Mode::SeparateOutputs;

        sat = self.sat;

        destinations = vec![match self.destination.clone() {
          Some(destination) => destination.require_network(chain.network())?,
          None => get_change_address(&client, chain)?,
        }];
      }
      (None, Some(batch)) => {
        let batchfile = Batchfile::load(&batch)?;

        parent_info = Inscribe::get_parent_info(batchfile.parent, &index, &utxos, &client, chain)?;

        postage = batchfile
          .postage
          .map(Amount::from_sat)
          .unwrap_or(TARGET_POSTAGE);

        (inscriptions, destinations) = batchfile.inscriptions(
          &client,
          chain,
          parent_info.as_ref().map(|info| info.tx_out.value),
          metadata,
          postage,
          self.compress,
        )?;

        mode = batchfile.mode;

        if batchfile.sat.is_some() && mode != Mode::SameSat {
          return Err(anyhow!("`sat` can only be set in `same-sat` mode"));
        }

        sat = batchfile.sat;
      }
      _ => unreachable!(),
    }

    let satpoint = if let Some(sat) = sat {
      if !index.has_sat_index() {
        return Err(anyhow!(
          "index must be built with `--index-sats` to use `--sat`"
        ));
      }
      match index.find(sat)? {
        Some(satpoint) => Some(satpoint),
        None => return Err(anyhow!(format!("could not find sat `{sat}`"))),
      }
    } else {
      self.satpoint
    };

    Batch {
      commit_fee_rate: self.commit_fee_rate.unwrap_or(self.fee_rate),
      destinations,
      changes,
      dry_run: self.dry_run,
      inscriptions,
      mode,
      no_backup: self.no_backup,
      no_limit: self.no_limit,
      parent_info,
      postage,
      reinscribe: self.reinscribe,
      reveal_fee_rate: self.fee_rate,
      satpoint,
    }
    .inscribe(chain, &index, &client, &locked_utxos, runic_utxos, &utxos)
  }

  fn parse_metadata(cbor: Option<PathBuf>, json: Option<PathBuf>) -> Result<Option<Vec<u8>>> {
    if let Some(path) = cbor {
      let cbor = fs::read(path)?;
      let _value: Value = ciborium::from_reader(Cursor::new(cbor.clone()))
        .context("failed to parse CBOR metadata")?;

      Ok(Some(cbor))
    } else if let Some(path) = json {
      let value: serde_json::Value =
        serde_json::from_reader(File::open(path)?).context("failed to parse JSON metadata")?;
      let mut cbor = Vec::new();
      ciborium::into_writer(&value, &mut cbor)?;

      Ok(Some(cbor))
    } else {
      Ok(None)
    }
  }

  fn get_parent_info(
    parent: Option<InscriptionId>,
    index: &Index,
    utxos: &BTreeMap<OutPoint, Amount>,
    client: &Client,
    chain: Chain,
  ) -> Result<Option<ParentInfo>> {
      Ok(None)
  }
}
