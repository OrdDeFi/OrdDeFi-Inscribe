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
OrdDeFi-Inscribe wallet --name [wallet_name] inscribe [--dry-run] --fee-rate [fee_rate] --destination [instruction_destination_address] --change [change_address] --file [path_of_instruction_file]
```

e.g.:  

```
OrdDeFi-Inscribe wallet --name orddefi inscribe --dry-run --fee-rate 36 --destination bc1pm8wv7dwnzs5dd6fhgdnurhhpat0zzgly6yugtr472nqhlxatlhdsq6t3ku --change bc1pm8wv7dwnzs5dd6fhgdnurhhpat0zzgly6yugtr472nqhlxatlhdsq6t3ku --file ~/inscription_demo/odfi_addlp.txt
```
