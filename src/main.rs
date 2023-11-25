use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use gloo::file::callbacks::FileReader;
use gloo::file::File;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use web_sys::{Event, FileList, HtmlInputElement};
use yew::html::TargetCast;
use yew::{html, Component, Context, Html};

#[derive(Deserialize)]
struct FileDetails {
    file_name: String,
    file_type: String,
    #[serde(deserialize_with = "deserialize_file_data")]
    data: Vec<u8>,
}

fn deserialize_file_data<'de, D>(d: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    Deserialize::deserialize(d).map(|v: String| STANDARD.decode(v.into_bytes()).unwrap())
}

struct App {
    server_url: String,
    readers: HashMap<String, FileReader>,
    satellite_image: Option<FileDetails>,
    mask_image: Option<FileDetails>,
}

enum Msg {
    AddNewImage(Vec<File>),
    FinishRead(String, String, Vec<u8>),
    FinishSend(Result<FileDetails, String>),
}

impl Component for App {
    type Message = Msg;

    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        let server_url = std::option_env!("SERVER_URL")
            .expect("No server url provided. Please set `SERVER_URL` environment variable.");
        Self {
            server_url: server_url.to_string(),
            readers: HashMap::default(),
            satellite_image: None,
            mask_image: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::AddNewImage(files) => {
                self.satellite_image = None;
                self.mask_image = None;
                log::info!("New image: {files:?}");
                for file in files.into_iter() {
                    let file_name = file.name();
                    let file_type = file.raw_mime_type();

                    let task = {
                        let link = ctx.link().clone();
                        let file_name = file_name.clone();

                        gloo::file::callbacks::read_as_bytes(&file, move |res| {
                            link.send_message(Msg::FinishRead(
                                file_name,
                                file_type,
                                res.expect("Failed to read file."),
                            ))
                        })
                    };
                    self.readers.insert(file_name, task);
                }
                true
            }
            Msg::FinishRead(file_name, file_type, data) => {
                log::info!("Finished reading {file_name}");
                self.readers.remove(&file_name);
                self.satellite_image = Some(FileDetails {
                    file_name: file_name.clone(),
                    file_type: file_type.clone(),
                    data: data.clone(),
                });
                let server_url = self.server_url.clone();
                ctx.link().send_future(async move {
                    let client = reqwest::Client::new();
                    let body = reqwest::multipart::Form::new().part(
                        "f[]",
                        reqwest::multipart::Part::bytes(data)
                            .file_name(file_name)
                            .mime_str(&file_type)
                            .unwrap(),
                    );
                    let reqwest = client
                        .post(format!("{}/segment", server_url))
                        .multipart(body)
                        .send()
                        .await;
                    let result = match reqwest {
                        Ok(resp) => match resp.error_for_status() {
                            Ok(mask) => match mask.json::<FileDetails>().await {
                                Ok(json) => Ok(json),
                                Err(e) => Err(format!("Error in receiving json: {e}")),
                            },
                            Err(e) => Err(format!("Error code in sending imaget to server: {e}")),
                        },
                        Err(e) => Err(format!("Error sending image to server: {e}")),
                    };
                    Msg::FinishSend(result)
                });
                true
            }
            Msg::FinishSend(resp) => {
                match resp {
                    Ok(mask) => self.mask_image = Some(mask),
                    Err(e) => log::error!("{}", e),
                };
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="row justify-content-evenly">
                <div class="col-4">
                    <h1>{"Satellite image"}</h1>
                    {
                        if let Some(file) = &self.satellite_image {
                            html! {
                                <div>
                                    <h2>{&file.file_name}</h2>
                                    <img
                                        width={"100%"}
                                        src={
                                            format!("data:{};base64,{}",
                                            file.file_type,
                                            STANDARD.encode(&file.data))
                                        }
                                    />
                                </div>
                            }
                        } else {
                            html! {
                                <p>{"No file uploaded."}</p>
                            }
                        }
                    }
                    <input
                        type="file"
                        accept="image/*"
                        multiple={false}
                        onchange={ctx.link().callback(move |e: Event| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            Self::upload_files(input.files())
                        })}
                    />
                </div>
                <div class="col-4">
                    <h1>{"Segments"}</h1>
                    {
                        if let Some(file) = &self.mask_image {
                            html! {
                                <div>
                                    <h2>{&file.file_name}</h2>
                                    <img
                                        width={"100%"}
                                        src={
                                            format!("data:{};base64,{}",
                                            file.file_type,
                                            STANDARD.encode(&file.data))
                                        }
                                    />
                                </div>
                            }
                        } else {
                            html! {
                                <p>{"No mask image."}</p>
                            }
                        }
                    }
                </div>
            </div>
        }
    }
}

impl App {
    fn upload_files(files: Option<FileList>) -> Msg {
        log::info!("Uploading new image");
        let mut to_upload = vec![];
        if let Some(files) = files {
            let files = js_sys::try_iter(&files)
                .unwrap()
                .unwrap()
                .map(|v| web_sys::File::from(v.unwrap()))
                .map(File::from);
            to_upload.extend(files);
        }
        Msg::AddNewImage(to_upload)
    }
}

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<App>::new().render();
}
