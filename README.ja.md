<!-- i18n: language-switcher -->
[English](README.md) | [日本語](README.ja.md)

# IoTDB コネクタ

IoTDB向けのネイティブIrodoriテーブルコネクタ拡張です。

このクレートは、Irodori拡張マーケットプレイスで使用されるコネクタのメタデータ、ネイティブABIエクスポート、およびドライバー実装をパッケージ化しています。

## コネクタ

- 拡張ID: `irodori.iotdb`
- エンジンID: `iotdb`
- ワイヤープロトコル: `timeSeries`
- デフォルトポート: `6667`
- ネイティブABI: `irodori.connector.native.v1`
- ドライバー連携: `あり`
- マーケットプレイス公開範囲: `公開`
- パッケージバージョン: `0.1.3`

このパッケージはコネクタのメタデータとネイティブドライバーを直接使用しており、デスクトップアダプターのソーススナップショットは不要です。

コネクタメタデータは `connector.config.json` と `irodori.extension.json` にあります。
Rustクレートは `src/lib.rs` からネイティブABIをエクスポートし、共有JSON/バッファヘルパーに `irodori-connector-abi` を使用し、コネクタの動作は `src/driver.rs` に保持しています。

## 接続メタデータ

- エンドポイントモード: `hostPort`, `connectionString`
- トランスポートモード: `direct`, `sshTunnel`, `socks5Proxy`, `httpConnectProxy`, `proxyChain`
- TLS対応: `あり`
- デフォルトでTLS必須: `いいえ`
- カスタムドライバーオプション: `あり`

### エンドポイントフィールド

| フィールド | ラベル | 型 | 必須 |
| --- | --- | --- | --- |
| `host` | ホスト | `string` | はい |
| `port` | ポート | `number` | いいえ |
| `database` | データベース | `string` | いいえ |

## 認証

コネクタはこれらの認証モードを宣伝し、クライアントが適切な認証情報フィールドを表示できるようにします。
ドライバー固有またはプロバイダー固有の値は必要に応じて `options` 経由で渡すことが可能です。

| 認証方式 | ラベル | 種類 | 秘密情報の用途 |
| --- | --- | --- | --- |
| `none` | 認証なし | `none` | なし |
| `connectionString` | 接続文字列 / DSN | `connectionString` | なし |
| `userPassword` | ユーザー/パスワード | `userPassword` | `password` |
| `bearerToken` | ベアラートークン | `token` | `token` |
| `kerberos` | Kerberos / GSSAPI | `kerberos` | `token` |
| `clientCertificate` | クライアント証明書 / mTLS | `certificate` | `privateKey`, `privateKeyPassphrase` |
| `customDriverOptions` | カスタムドライバーオプション | `custom` | `password`, `token`, `privateKey`, `privateKeyPassphrase` |

## エクスペリエンスメタデータ

- ドメイン: `timeSeries`
- 結果ビュー: `timeChart`, `table`, `heatmap`
- オブジェクトタイプ: `storageGroups`, `devices`, `measurements`, `templates`, `ttl`
- インスパイア元: Apache IoTDBの時系列階層、GROUP BY time、ALIGN BY DEVICE、FILL

| ワークフロー | 結果ビュー | テンプレート |
| --- | --- | --- |
| デバイス整列クエリ | `timeChart` | `time-iotdb-group-by-device` |
| ギャップフィル | `timeChart` | `time-iotdb-fill` |
| 最新テレメトリ | `table` | `time-iotdb-latest` |

| テンプレート | ラベル | 言語 | 結果ビュー |
| --- | --- | --- | --- |
| `time-iotdb-group-by-device` | デバイスごとに整列したGROUP BY time | `sql` | `timeChart` |
| `time-iotdb-fill` | 欠損ウィンドウの補完 | `sql` | `timeChart` |
| `time-iotdb-latest` | 最新値 | `sql` | `table` |

## ネイティブABIコール

| メソッド | レスポンス |
| --- | --- |
| `health` | コネクタのヘルス、エンジンID、ABIバージョン、ドライバー状態を返します。 |
| `describe` | 埋め込みマニフェストとコネクタ設定を返します。 |
| `manifest` | 生の `irodori.extension.json` を返します。 |
| `config` | 生の `connector.config.json` を返します。 |
| `connect` | ネイティブコネクタ接続を開き、検証します。 |
| `query` | コネクタクエリを実行し、構造化された行またはJSON結果を返します。 |
| `metadata` | スキーマ、テーブル、カラム、インデックス、コレクション、または同等のメタデータを読み取ります。 |
| `close` | キャッシュされたネイティブ接続を閉じて削除します。 |

## 開発

このチェックアウト内のすべての拡張クレートは `../target` を共有しているため、依存関係は兄弟リポジトリ間で一度だけコンパイルされます。

```sh
make check
make build
```

リリースパッケージはプラットフォーム固有のネイティブアーティファクトを `dist/native` に配置します。

## ライセンス

0BSD。ほぼあらゆる目的でこのプロジェクトを使用、コピー、修正、配布できます。