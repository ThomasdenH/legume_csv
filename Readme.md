# legume_csv
[![Actions Status](https://github.com/ThomasdenH/legume_csv/workflows/Rust/badge.svg)](https://github.com/ThomasdenH/legume_csv/actions)

A tool to create beancount files from csv files.

To work, it requires an input file (`--ledger`, `-l`) and a configuration (`--config`, `-c`). Optionally, you can use `--append` to specify a file to which the new entries will be appended.

## Configuration
For example configurations, see the `configs` folder.

The idea is simple but versitile. Using the `input` key, the columns in the `csv` file can be given a name. The `output` key can then be used to specify the different items of a transaction. These items are formatted using Handlebars templates.

## License
Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license
  ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
