extern crate actix;
extern crate actix_web;
extern crate env_logger;
extern crate futures;
extern crate serde_json;
extern crate reqwest;
extern crate scraper;
extern crate slack_hook;
extern crate yaml_rust;
extern crate image;

use std::env;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::fs::{File, remove_file};
use std::io::{BufReader, Read, BufWriter, Write};
use reqwest::Client;
use reqwest::multipart::Form;
use yaml_rust::{Yaml, YamlLoader};
use futures::{future::ok as fut_ok, Future};
use actix_web::{http::Method, server, App, Error, HttpResponse, HttpMessage, HttpRequest};
use serde_json::Value;

const FILE_NAME: &str = "emoji.png";

#[derive(Debug)]
struct SlackConfig {
    workspace_name: Yaml,
    api_token: Yaml
}

impl SlackConfig {
    fn new() -> SlackConfig {
        // yml読み込み
        let mut f = BufReader::new(File::open("config.yml").unwrap());
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        let docs = YamlLoader::load_from_str(&mut s).unwrap();

        SlackConfig {
            workspace_name: docs[0][0].clone(),
            api_token: docs[0][1].clone(),
        }
    }
}

// 絵文字のアップロードを行う
fn upload_emoji(url: &str, form: Form, emoji_name: &str, api_token: &str)
-> impl Future<Item = String, Error = Error> {
    // カスタム絵文字アップロードリクエスト
    let mut res = Client::new()
        .post(url)
        .query(&[
            ("mode", "data"),
            ("name", emoji_name),
            ("token", api_token)])
        .multipart(form)
        .send()
        .unwrap();

    match res.text() {
        Err(e) => fut_ok(String::from(format!("アップロード失敗:{:?}", e))),
        Ok(result) => {
            let v: Value = serde_json::from_str(result.as_str()).unwrap();

            // エラー判定
            match v["ok"].as_bool() {
                Some(true) => fut_ok(String::from(format!("アップロード完了::{}:", emoji_name))),
                Some(false) => fut_ok(String::from(format!("アップロード失敗:{}", v["error"]))),
                None => fut_ok(String::from(format!("アップロード失敗:{}", v["error"]))),
            }
        }
    }
}

/// URLから画像を取得してローカルに保存する
fn download_image(url: &str) {
    // 画像データの取得
    let image = reqwest::get(url).unwrap();
    let file = File::create(FILE_NAME).unwrap();
    let mut br = BufWriter::new(&file);
    for byte in image.bytes() {
        br.write(&[byte.unwrap()]).unwrap();
    }
    br.flush().unwrap();

    // 保存した画像に読み取り権限を付与
    let mut perms = file.metadata()
        .expect("メタデータの取得に失敗しました。")
        .permissions();
    perms.set_readonly(true);
    file.set_permissions(perms)
        .expect("権限付与に失敗しました。");
}

fn upload_process(req: HttpRequest) -> impl Future<Item = HttpResponse, Error = Error> {
    req.urlencoded::<HashMap<String, String>>()
        .from_err()
        .and_then(|params| {
            // 空白で文字列を分割する 絵文字画像URL 登録絵文字名
            let v: Vec<&str> = params.get("text").unwrap().split(' ').collect();
            let emoji_url: &str = v.get(0).unwrap();
            let emoji_name: &str = v.get(1).unwrap();

            // アップロードする画像の取得
            download_image(emoji_url);

            // ymlデータ取得
            let slack_config = SlackConfig::new();
            // slackへの画像アップロード用リクエストURLを作成
            let slack_url_add = format!(
                r#"https://{}.slack.com/api/emoji.add"#,
                    slack_config.workspace_name.into_string().unwrap());
            // 画像アップロード用リクエストを生成
            let form = Form::new()
                .file("image", FILE_NAME)
                .expect("画像ファイルを開けませんでした。");

            // 絵文字アップロード
            upload_emoji(slack_url_add.as_str(), form, emoji_name, slack_config.api_token.as_str().unwrap())
                .and_then(|d| {
                    // ファイル削除
                    remove_file(FILE_NAME)
                        .expect("ファイル削除に失敗しました。");

                    Ok(HttpResponse::Ok()
                        .content_type("application/json")
                        .body(serde_json::to_string(&d).unwrap())
                        .into())
                })
    })
}

fn get_server_port() -> u16 {
    env::var("PORT").ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(6969)
}

fn main() {
    ::std::env::set_var("RUST_LOG", "actix_web=info");
    env_logger::init();
    let sys = actix::System::new("custom_emojikun");

    let addr = SocketAddr::from(([0, 0, 0, 0], get_server_port()));

    server::new(move || {
        App::new()
            .resource("/custom_emojikun/upload", |r| {
                r.method(Method::POST).with_async(upload_process)
            })
    }).bind(addr)
        .unwrap()
        .start();

    println!("Started http server");
    let _ = sys.run();
}
