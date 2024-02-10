`OrdDeFi-Inscribe`
=====

`OrdDeFi-Inscribe` is a command-line wallet and instruction inscriber for OrdDeFi, forked from [`ord`](https://github.com/ordinals/ord). It is experimental
software with no warranty. See [LICENSE] for more details.

Requirements
------
* Install [Bitcoin Core](https://github.com/bitcoin/bitcoin/releases) v24.0.1 or above;
* Install bitcoin-cli v24.0.1 or above;
* Install [Rust](https://www.rust-lang.org).


Build
------

```
git clone git@github.com:OrdDeFi/OrdDeFi-Inscribe.git
cd OrdDeFi-Inscribe
cargo build --release
```

Install 
------

```
mkdir ~/bin
export PATH="$HOME/bin:$PATH"
cp target/release/OrdDeFi-Inscribe ~/bin
```

Create Wallet
------

Create a wallet named "orddefi":  

```
OrdDeFi-Inscribe wallet --name orddefi create
```

Generate address to receive fee (gas):  

```
OrdDeFi-Inscribe wallet --name orddefi receive
```


Inscribe Instructions
------

```
OrdDeFi-Inscribe wallet --name [wallet_name] inscribe [--dry-run] --fee-rate [fee_rate] --origin [instruction_source_address] --destination [instruction_destination_address] --change [change_address] --file [path_of_instruction_file]
```

### Params:

* --name: The wallet name in `Bitcoin Core`, equivalent to `-rpcwallet=` in `bitcoin-cli`.
* --dry-run: This option prevents the transaction from being broadcasted. It only signs the transaction and displays the raw commit tx and reveal tx.
* --fee-rate: The fee rate for the commit tx and reveal tx. Please note that the `OpReturn` output in the commit tx is not included in this calculation, so the effective fee rate may be lower than the input parameter.
* --origin: The address from which the instruction is executed. The fees associated with the transaction should be deducted from this address.
* --destination: The address on which the instruction is executed. The controlling OrdDeFi assets should be present in this address.
* --change: Specifies the address where the change is sent after deducting the fees.
* --file: The local path of the file that stores the instruction JSON file.

Warning: when inscribing `mint`, `addlp`, `rmlp`, `swap` and `direct-transfer` (`transfer` with `to` param), `--origin` param should be same as `--destination` for authentication. Otherwise the instruction will be aborted.

### Inscribe command example:  

```
OrdDeFi-Inscribe wallet --name orddefi inscribe --dry-run --fee-rate 36 --origin bc1pm8wv7dwnzs5dd6fhgdnurhhpat0zzgly6yugtr472nqhlxatlhdsq6t3ku --destination bc1pm8wv7dwnzs5dd6fhgdnurhhpat0zzgly6yugtr472nqhlxatlhdsq6t3ku --change bc1pm8wv7dwnzs5dd6fhgdnurhhpat0zzgly6yugtr472nqhlxatlhdsq6t3ku --file ./inscription_demo/insc.txt
```

### Instruction Examples

See the [instruction_demo](https://github.com/OrdDeFi/OrdDeFi-Inscribe/tree/main/instruction_demo) files.
