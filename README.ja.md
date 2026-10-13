<div align="center">
  <img src="https://raw.githubusercontent.com/hibanaworks/hibana/v0.9.9/.github/assets/hibana-header.svg" width="600" alt="HIBANA — Choreographic Programming in Rust" />

  <p>
    <a href="https://github.com/hibanaworks/hibana/actions/workflows/quality-gates.yml"><img src="https://github.com/hibanaworks/hibana/actions/workflows/quality-gates.yml/badge.svg" alt="Quality gates" /></a>
    <a href="https://crates.io/crates/hibana"><img src="https://img.shields.io/crates/v/hibana.svg" alt="crates.io" /></a>
    <a href="https://docs.rs/hibana"><img src="https://docs.rs/hibana/badge.svg" alt="docs.rs" /></a>
    <a href="LICENSE-APACHE"><img src="https://img.shields.io/crates/l/hibana.svg" alt="Apache-2.0 OR MIT" /></a>
  </p>
</div>

# Hibana — Choreographic Programming in Rust

[English](README.md)

Hibanaは、通信の流れをまとめて記述するchoreographic programmingのためのRustライブラリです。
誰が、誰に、何を、どの順序で送るかを、参加者共通のプロトコルとして書きます。
参加者は同じプログラム内のタスクでも、別のマシンで動くプログラムでも構いません。

各参加者のRustコードも自分で書きます。Hibanaは共通のプロトコルから各参加者の通信手順を導出し、
実行時に、その送受信が手順に従っているかを検査します。

コアは`no_std`で、ヒープアロケータを必要としません。stable Rust 1.95以降で動作します。
ランタイムが使うメモリは、プログラム側から渡します。

[hibana-quic](https://github.com/hibanaworks/hibana-quic)は、Hibanaを使ったQUIC v1/v2とHTTP/3の実装です。
接続内部の通信にも、QUIC経由でやり取りするアプリケーションのメッセージにも、同じ方法を使います。

## プロジェクトに追加する

プロジェクトのディレクトリで実行します。

```sh
cargo add hibana@0.9.9
```

`Cargo.toml`に次の依存関係が追加されます。

```toml
[dependencies]
hibana = "0.9.9"
```

## 小さな通信例

クライアントが数値を送り、サーバーが1を足して返します。クライアントは返信を受け取る前に送信する必要があります。
サーバーは返信する前に受信する必要があります。

この順序を一度だけ書きます。

```rust
use hibana::g;

const CLIENT: u8 = 0;
const SERVER: u8 = 1;
type Request = g::Msg<1, u32>;
type Reply = g::Msg<2, u32>;

let exchange = g::seq(
    g::send::<CLIENT, SERVER, Request>(),
    g::send::<SERVER, CLIENT, Reply>(),
);
```

`Request`と`Reply`はどちらも`u32`を運びます。ラベル`1`と`2`でメッセージの種類を区別します。
`g::seq`は、requestの後にreplyが来ることを表します。

各参加者には、自分の通信手順を実行するためのエンドポイントが渡されます。
クライアントは`7`を送り、返信を待ちます。

```rust
client.send::<Request>(&7).await?;
let answer = client.recv::<Reply>().await?;
```

サーバーは数値を受け取り、1を足して返します。

```rust
let number = server.recv::<Request>().await?;
server.send::<Reply>(&(number + 1)).await?;
```

これらはコードの抜粋です。
[完全な例](https://github.com/hibanaworks/hibana/blob/main/examples/ping_pong.rs)では、
メモリの用意、メモリ内トランスポートの設定、両エンドポイントの接続も行います。
hibana-quicなどのライブラリは、アプリケーション用エンドポイントに対してこの設定を行います。

`Request`を送る前に`client.recv::<Reply>()`を呼んでも、Rustコードとしてはコンパイルできます。
ただし受信の順序が違うため、Hibanaが実行時にエラーを返します。
Hibanaは、操作を完了する前にメッセージの種類とペイロードの形式も検査します。

## 試す

リポジトリを複製して、例を実行します。

```sh
git clone https://github.com/hibanaworks/hibana.git
cd hibana
cargo +1.95.0 run --locked --example ping_pong
```

出力は次のとおりです。

```text
ping=7, pong=8
```

プロトコルには、分岐、独立した通信の並列合成、繰り返しも記述できます。
参加者側では、同じ`send`、`recv`、`offer`を使います。

## QUICで使う

[hello-quicの例](https://github.com/hibanaworks/hibana-quic/tree/main/examples/hello-quic)では、
クライアントがQUIC ストリームで数値を送り、サーバーがその平方を返します。
[global protocol](https://github.com/hibanaworks/hibana-quic/blob/main/examples/hello-quic/global.rs)に、
両側の通信を定義しています。クライアントとサーバーは、それぞれエンドポイントで自分の手順を実行します。

Hibanaはhibana-quicの内部通信にも使われています。
[QUICのプロトコル](https://github.com/hibanaworks/hibana-quic/blob/main/src/quic/global.rs)には、
パケット処理、TLS、送信、タイマーの間の通信を定義しています。
各ロールを実装するRust関数は、エンドポイントを使ってこの手順に従います。
UDP I/Oと暗号処理はhibana-quicとhibana-tlsが実装します。

hibana-quicは実験的な実装です。コアのプロトコル証明は、QUICやTLS実装そのものの正しさを証明していません。

## 仕組み

### Global protocolとlocal protocol

Multiparty session types（MPST）では、**global protocol**が全参加者の通信を表します。
**role**は、そのプロトコルの参加者を表します。
**endpoint projection**は、global protocolから各ロールの**local protocol**を導出します。

先ほどの例をprojectionすると、次の手順になります。

| Global protocolの通信 | クライアントのlocal protocol | サーバーのlocal protocol |
| --- | --- | --- |
| クライアントがサーバーに`Request`を送る | `Request`を送信 | `Request`を受信 |
| サーバーがクライアントに`Reply`を送る | `Reply`を受信 | `Reply`を送信 |

あるメッセージに参加しないロールには、そのメッセージの送受信は含まれません。
残った操作の順序と依存関係は、逐次合成と並列合成から決まります。

Hibanaの着想は[affine multiparty session types（AMPST）](https://arxiv.org/abs/2204.13464)です。
ここで*affine*とは、通信のためのリソースを高々一度使えることを意味します。
プロトコルを完了する前に破棄することもできます。

AMPSTの実装であるMultiCrustyは、local session typeをRustの型で表します。
Hibanaでは、プロトコルが大きくなるにつれて、残りの通信手順をすべてエンドポイントの型で表すと型が複雑になりました。
そこでlocal protocolをデータにコンパイルする形にしています。
エンドポイントは操作ごとにRustの型を変える代わりに、そのデータ上で実行位置を進めます。

メッセージとglobal protocol自体には、引き続きRustの型を使います。
そのため、元のプロトコルが大きくなると、型も複雑になり得ます。

### Projection

Global protocolは、型を持つゼロサイズの値です。`project`は一つのロールのプロトコルデータを返します。

```rust
use hibana::runtime::program::project;

let client_program = project::<CLIENT>(&exchange);
let server_program = project::<SERVER>(&exchange);
```

ProjectionはRustのconst評価で、このデータを構築して検証します。
実装には関連定数を使うため、上の呼び出しを`const`宣言に書く必要はありません。

`RoleProgram`は、送受信、通信相手、メッセージラベル、ペイロードのスキーマ、依存関係、分岐への所属を記録します。
どの通信チャネルが独立して進められるかも記録します。
トランスポートAPIでは、このチャネルを**lane**と呼びます。

Projectionは、ロールが必要な選択を知ることができるか、並列の操作を区別できるかを検査します。
この検査を満たさないプロトコルは、コンパイル時に拒否します。

Projectionが生成するのは通信のデータです。参加者の関数と、その中の計算は自分でRustコードとして書きます。
Global protocolはコンパイル時に検査します。
各関数の送受信は、実行時にlocal protocolと照合します。

### Selection、branching、knowledge of choice

**selection**は、選択するロールの操作です。
**branching**は、別のロールがその選択に応じて分岐することです。
**knowledge of choice**は、ロールが分岐によって異なる動作をする前に、どの分岐が選ばれたかを知れることです。

メッセージによる選択では、両分岐の選択者が同じロールである必要があります。
ほかの参加ロールは、受信メッセージから選択を知ります。
Projectionは、それぞれのロールのlocal protocolを両分岐について調べます。
共通の受信操作が先にあっても構いませんが、そのロールが送信する前に、区別できる受信操作が必要です。
両分岐に参加しないロールへの通知は不要です。一方の分岐にしか参加しないロールがある選択は拒否します。

現在の規則は保守的で、MPSTのprojectionで認められるmergeの一部だけを扱います。
例えば、送信以降の手順が同一であるすべてのケースを受け入れるわけではありません。
この規則で検査できない選択は拒否します。Hibanaが一斉通知を挿入することはありません。
第三のロールに選択を知らせる必要があるなら、そのロールへのメッセージをglobal protocolに書きます。

実行時には、受信フレームから許可された分岐を識別します。
`offer()`は`RouteBranch`を返し、参加者はプロトコルを進めずに選択された分岐を確認できます。
`label()`は、その分岐の最初の論理メッセージラベルを返します。
分岐の最初の送受信が成功すると、プロトコルが進みます。
確認するだけ、またはプレビューを破棄するだけでは進みません。

選択に`.resolve::<ID>()`を付けると、アプリケーションの決定関数に接続できます。
選択の方法は変わりますが、ほかのロールのprojection検査を省略したり、別のマシンへ決定を伝えたりはしません。
選択を知らせるメッセージの受信より前に合意が必要なら、アプリケーションか配布・設定がその合意を用意します。

### 並列通信と繰り返し

`g::par(left, right)`は、二つの部分に独立した通信を記述します。
Projectionは所有権を検査し、それぞれの操作を区別します。
両側とも、先行する逐次操作への依存を保持します。
一方の途中経過が、他方の依存関係に入ることはありません。
後続の逐次操作は、そのロールに必要な並列操作の完了を待ちます。
Rust関数の実行をスケジュールするのはexecutorです。

`body.roll()`は、完了したプロトコルの一部分を再び実行できるようにします。
Projectionはguarded repetitionを要求します。
繰り返しではコンパイル済みデータを再利用し、その部分の操作を再度実行できます。
新しいメッセージと古いメッセージを区別するには、FIFO配送、リプレイの排除、セッションの実行間の分離が必要です。
繰り返すプロトコルは、無期限に実行し続ける場合があります。

### Local protocolの実行

接続されたエンドポイントは、一つのセッションの一つのロールのストレージを借用します。
複製して完了済みの操作を再実行することはできません。
操作が成功するたびに、local protocolが一度進みます。

送信では、要求された操作とメッセージのスキーマを検査してから、フレームをトランスポートに渡します。
受信では、フレームを許可されたlocal protocolの操作と照合し、ペイロードの正確な形式を検証してから進めます。

非同期送信の成功は、トランスポートがフレームを受け入れたことを意味します。
相手が受信したことを意味するわけではありません。

Hibanaは、手書きの参加者関数が行う通信を検査します。
実行中のタスクを観察してデッドロックの循環を検出・修復する仕組みはありません。
Protocol deadlock freedomには、検査済みプロトコルと
[保証と検証](#保証と検証)に示す実行の前提が必要です。

## 仕様

### プロトコル言語とエンドポイント操作

| 記述 | 意味 |
| --- | --- |
| `g::send::<FROM, TO, g::Msg<LABEL, PAYLOAD>>()` | 一つの非同期メッセージ |
| `g::seq(left, right)` | 逐次合成 |
| `g::par(left, right)` | 独立した並列合成 |
| `g::route(left, right)` | 二者択一の分岐 |
| `route.resolve::<ID>()` | 選択にアプリケーションの決定関数を使う |
| `body.roll()` | Guarded repetition |

名前を付けたプロトコルは、`impl Projectable`を返す関数として書けます。
このトレイトの実装は、Hibanaのプロトコルコンストラクターに限定されています。
プロトコルを合成してから、各ロールにprojectionします。

論理メッセージラベルが、プロトコル全体で一意である必要はありません。
送信元と送信先が同じロールの`send`は、ゼロバイトのlocal操作です。
ペイロードにはunit型の`()`を使います。

Projectionは、不正または観測できない選択、曖昧な同時操作、並列部分の所有権の競合、
空の並列部分、unguarded repetitionを拒否します。
同じlaneで連続する受信の送信者が変わる場合は、因果順序も検査します。

| 操作 | 結果 |
| --- | --- |
| `Endpoint::send::<M>(&payload)` | 許可されたメッセージをトランスポートが受け入れると進む |
| `Endpoint::recv::<M>()` | 許可されたメッセージを検証し、デコードしたペイロードを返す |
| `Endpoint::offer()` | プロトコルを進めず、分岐のプレビューを返す |
| `RouteBranch::label()` | 選ばれた分岐の最初の論理メッセージラベルを返す |
| `RouteBranch::send` / `recv` | プレビューを消費し、その分岐の最初の可視操作を実行する |

操作がlocal protocolを進めることを**コミット**と呼びます。
分岐のプレビューの破棄、コミット前の操作の破棄、操作の拒否ではプロトコルは進みません。
コミットしていない受信フレームのrequeueでも進みません。

### ペイロード

`g::Msg<L, P>`は、論理ラベル`L`にペイロード型`P`を対応付けます。
組み込みコーデックは、unit、真偽値、整数、バイト スライス、固定長バイト配列を扱います。
固定長デコーダーは余分な末尾バイトを拒否します。
デコードした値は、トランスポートの受信バッファを借用できます。

独自ペイロードには、`hibana::runtime::wire`の`WireEncode`と`WirePayload`を実装します。
エンコードは、同じ値を同じバイト列に変換し、渡されたストレージに書きます。
検証は、コミットする前に定めた形式を正確に検査します。
デコードは、検証済みのバイト列を読みます。

`WirePayload::SCHEMA_ID`は、wire形式とその検証規則を識別します。
Rustの型を識別するものではなく、コアのフレームヘッダーにも送信されません。
両側はエンコードと検証規則に合意する必要があります。
同じIDを付けるだけでは、その合意は成立しません。
異なる形式には異なるスキーマIDが必要です。スキーマ `0`はゼロバイトのunit形式です。

### トランスポート

トランスポートはバッファ、フレームの形式と境界、I/Oの準備状態、起床通知を提供します。
`hibana::runtime::transport`を通して、借用された送受信ハンドルを渡します。

| 操作 | 契約 |
| --- | --- |
| `open(PortOpen)` | ハンドルをprojectionされたロール、セッション、laneに結び付ける |
| `poll_send` | 一つのフレームを受け入れるか、渡されたペイロードのポインターを保持せずpendingを返す |
| `cancel_send` | 待機中に破棄された送信の状態を取り消す。バイトを取り消せない場合は、その論理送信方向を残りの実行で使えなくする |
| `poll_recv` | 一つの借用フレームを返すか、現在のwakerを記録してpendingを返す |
| `requeue` | 直前に受信した未コミットのフレームを、同じ受信ハンドルに戻す |

再度の送信pollでは同じ内容が渡されますが、バッファのアドレスは変わり得ます。
I/Oの待機状態は、一時的なペイロードのバッファではなくトランスポートが保持します。
破棄した送信のバイトが、後の送信で送られてはいけません。

ヘッダー付きの受信では、次の8バイトを使います。

```text
session id (4 bytes, big endian) | lane | source role | target role | frame label
```

このバイト列はトランスポートが所有します。
フレームラベルはprojectionが割り当てるwire上の識別子で、`Msg`の論理メッセージラベルとは別です。
ランタイムはヘッダーと要求されたメッセージをlocal protocolと照合します。

`ReceivedFrame::deterministic`にはヘッダーがありません。
Laneに結び付けられた受信と要求されたメッセージが、実行可能な受信をちょうど一つ識別する場合、
または分岐のプレビューが既にその受信を識別している場合にだけ使えます。
未解決の分岐には`ReceivedFrame::framed`が必要です。

全体の実行に関する保証には、トランスポートがメッセージを期待する相手と方向に結び付け、各方向のFIFO順序を保ち、
意図しないリプレイや以前のセッション実行のメッセージが現在の実行に入ることを防ぐ必要があります。
受信メッセージを複製してはいけません。
未コミットのフレームを`requeue`で戻すことも、二つ目のメッセージとして扱ってはいけません。

受け入れられたフレームは、最終的に届くか、その方向の終了を観測できる必要があります。
受け入れ済みフレームを配送し終えるか隔離した後、終了によって待機中の受信者を起こし、エラーを返します。
終了が分かっている方向が、永久にpendingのままになってはいけません。

これはRustのインターフェースだけではなく、トランスポート実装に対する要件です。
[Unix datagramの例](https://github.com/hibanaworks/hibana/tree/main/proofs/unix-carrier)で、
一つの実装についてこの要件をテストしています。

### 期限、エラー、cancellation

Hibanaには組み込みの時計や障害検知器はありません。
I/O操作が期限を超えたかどうかはトランスポートが判断し、`TransportError::Deadline`を返します。
ほかのトランスポートエラーは`Offline`、`Capacity`、`Failed`です。

これらのエラーで、現在のセッション実行は終了します。エンドポイント操作は`EndpointError`を返します。
ランタイムは最初の障害を保持し、同じランタイム内の待機者を起こします。
失敗した操作ではプロトコルは進みません。
自動再試行、期限延長、別の分岐への切り替えは行わず、この実行を続けることもできません。

タイムアウトやcancellationの後にもプロトコルの通信を続けたい場合は、
それをglobal protocolのメッセージか明示的な選択として記述します。
トランスポートエラーの後に再試行するなら、新しいセッション実行を開始します。
トランスポートは受け入れ済みフレームを配送し終えるか隔離し、新しい実行に届かないようにします。

待機中の送信を破棄すると、`cancel_send`が呼ばれます。
トランスポートは送信待ちのバイトを破棄します。
取り消せないバイトがある場合は、その論理送信方向を残りの実行で使えなくする必要があります。
同じランタイム内の障害だけでは、別のマシンのプロセスに通知できません。
Distributed cancellationには、相手が終了を観測できるトランスポートも必要です。

| エラーが起きる場所 | エラー型 |
| --- | --- |
| ストレージの設定とエンドポイントの接続 | `AttachError` |
| 決定関数の登録または呼び出し | `ResolverError` |
| トランスポートの送信、受信、requeue | `TransportError` |
| エンドポイントまたは分岐の操作 | `EndpointError` |

### セッションとストレージ

セッションは固定されたプロトコルと、固定された有限のロール集合を持ちます。
**セッションの実行**は、そのセッションを一度実行することです。
失敗の後の再実行は、同じ`SessionId`を再利用しても新しい実行です。

接続を設定するコードは`hibana::runtime`でロールをprojectionし、メモリとトランスポートを用意して、エンドポイントを接続します。
`SessionKitStorage`がランタイムのストレージを保持します。
`rendezvous`にメモリ領域とトランスポートを渡し、`enter`でprojectionされたロールを`SessionId`に接続します。
メモリ領域が不足する場合や接続に失敗する場合は、部分的に接続されたエンドポイントを返さず、`AttachError`を返します。

セッションごとに進行状態と失敗状態は分かれています。
一つのランタイム内では、同じセッションの同じロールに二つの有効なエンドポイントを接続できません。
既にプロトコルが結び付けられたセッションに、異なるプロトコルバイトを渡すことも拒否します。
別のマシン間では、配布・設定時に、両側が同じプロトコルを使うことを保証する必要があります。
この合意のためにHibanaがハンドシェイクを行うことはありません。

`SessionId`の再利用には、前の実行のフレームが現れない新しいトランスポート状態が必要です。
参加者集合やプロトコルを変える場合は、新しいセッションが必要です。
トランスポートの保証が保たれるなら、通信アドレスの移行は同じ実行の中で行えます。
このモデルはチャネル委譲を扱いません。

ランタイムは、呼び出し側から渡されたストレージと借用バッファを使い、ヒープへのフォールバックはありません。
エンドポイントはローカルなランタイムに結び付けられ、`Send`でも`Sync`でもありません。
別のランタイムとの通信にはトランスポートを使います。
スケジューリング、永続的なアプリケーション状態、再実行の方針はアプリケーションが担当します。

### 容量の制限

| 項目 | 制限 |
| --- | ---: |
| ロール識別子 | `0`から`255`までの256種類 |
| イベント識別子 | 65,535種類。`u16::MAX`は不在を表すために予約 |
| コンパイル済みglobal protocol | 65,535バイト |
| コンパイル済みlocal protocol | ロールごとに65,535バイト |
| メッセージだけのglobal protocol | 1イベントにつき9バイトで、最大7,281イベント |
| 分岐と繰り返しの部分 | プロトコルのバイト数で制限。各部分に最低10バイト |
| 決定関数の識別子 | `u16`全範囲の65,536種類 |
| トランスポートのlane | 256 |
| 競合するwire識別子 | 同じ送信元、受信先、laneについて256種類 |

Global protocolとlocal protocolの両方が、バイト数の制限内に収まる必要があります。
分岐、繰り返し、依存関係にもバイトを使うので、イベント識別子の範囲だけでは格納できるイベント数は決まりません。
分岐のメタデータも、このバイト数に含まれます。

Wire識別子は、競合し得る受信を区別します。
順序が決まったメッセージや異なる送信元のメッセージでは、識別子を再利用できます。
256種類という制限は、一つのプロトコルのメッセージ数を256に制限するものではありません。
Projectionは通信に応じてlaneを割り当て、所有権が競合しなければ再利用できます。

元のプロトコルはRustの型の木です。大きな生成プロトコルでは、偏りの少ない合成を使います。
深く入れ子になったプロトコルには、より大きなRustの`recursion_limit`が必要な場合があります。
ソースコードのコンパイル制限とランタイムのストレージ制限は別です。

## 保証と検証

ランタイムは、各エンドポイントの操作がlocal protocolに従うかを検査します。
コミット前に方向、通信相手、lane、メッセージの識別、ペイロードのスキーマを検査します。
エンドポイントは、次に進むまでに一つのlocal操作を高々一度完了できます。
繰り返しでは、その操作が再び実行可能になります。失敗した操作でプロトコルは進みません。

Leanの証明は、そのモデルについてsession fidelity、protocol progress、cancellationの性質を示します。
全体の実行に関する結果は、独立した検査器に受理された、正確な生成プロトコルデータに適用します。
Rustのprojectionが成功しただけでは、distributed deadlock freedomの証明にはなりません。

実行中のシステムにこれらの結果を適用するには、次の前提が必要です。

1. 各ロールが、同じglobal protocolから導出されて受理されたlocal protocolに従う。
2. 両側がwire形式に合意し、その形式に従うコーデックを使う。
3. トランスポートが、選んだモデルに必要な通信相手との結び付け、順序、リプレイ排除、観測可能な終了を提供する。
4. Executorが、実行可能な状態を保っている操作を最終的にpollする。
5. ランタイム実装が、モデルの操作と所有権の規則に従う。

この前提の下では、実行中で未完了のプロトコルに、実行可能な通信遷移があります。
これが**一つのセッション内のprotocol deadlock freedom**です。
繰り返す通信の終了、任意のRust計算内部のデッドロックの不在、別々のセッション間の依存循環の不在までは示しません。

Cancellationの結果は、定められた有限のトランスポート状態と、方向を使えなくする規則を対象にします。
期限と障害検知はトランスポートについての前提です。
分岐の合意には、選択するロールもエンドポイントに従うことが必要です。
参加者が相手ごとに矛盾する選択を送る場合、認証だけで合意を保証することはできません。

[Leanの証明の説明](https://github.com/hibanaworks/hibana/tree/main/proofs/lean)に、正確な主張と前提を記載しています。
Rustの全行や、すべての`Transport`実装を検証したものではありません。
Kaniは有界なカーネルのケースを検査します。
Miriは実行したテストについて、借用、ポインターの由来、破棄の動作を検査します。
Rustのテストはprojectionの拒否、操作順序、ペイロード検証、トランスポート契約を確認します。
RustとLeanの比較は、生成したテストケースについて一致を確認します。

## 参考文献

直接の着想は、Lagaillardie、Neykova、Yoshidaによる
[Stay Safe under Panic: Affine Rust Programming with Multiparty Session Types](https://arxiv.org/abs/2204.13464)
（ECOOP 2022）です。

## ビルドと検査

クレートはRust 2024を使い、デフォルトのfeatureは空で、通常の依存クレートはありません。
リポジトリのルートで次を実行します。

```sh
cargo +1.95.0 check --no-default-features --lib -p hibana
cargo +1.95.0 test -p hibana
cargo +1.95.0 doc -p hibana --no-deps --no-default-features
```

[CIワークフロー](https://github.com/hibanaworks/hibana/blob/main/.github/workflows/quality-gates.yml)では、
Lean、Kani、Miri、組み込み向けビルドも実行します。
コードサイズ、スタックとストレージの使用量、エンドポイントとfutureの配置・サイズも検査します。
測定値は、その検査対象のプログラムとビルドについての値です。
メモリ使用量はプロトコルと設定によって変わります。

## ライセンス

HibanaはApache-2.0またはMITの、いずれかを選んで利用できます。
