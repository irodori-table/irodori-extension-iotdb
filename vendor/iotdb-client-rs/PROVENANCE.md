# IoTDB client dependency compatibility patch

Source: crates.io iotdb-client-rs 0.3.12, Apache-2.0. Registry archive SHA256: `81b17be6fd76f279e042783f26ae07f0f915b665fda80301916ae96c2cdb8491`.

Only the normalized Cargo manifest changes: Thrift 0.17 is replaced with 0.23, which fixes CVE-2026-43868. Generated protocol source is unchanged. The client keeps its original version and license. Remove this compatibility patch when the upstream client allows fixed Thrift.
