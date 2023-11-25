use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use gloo::file::callbacks::FileReader;
use gloo::file::File;
use serde::{Deserialize, Deserializer};
use shadow_clone::shadow_clone;
use std::{borrow::Borrow, collections::HashMap, rc::Rc, sync::Arc};
use web_sys::{Event, FileList, HtmlInputElement};
use yew::{
    prelude::*,
    suspense::{use_future, use_future_with},
};
use yew_autoprops::autoprops_component;
use yew_hooks::prelude::*;

#[derive(Deserialize, PartialEq, Clone)]
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

#[function_component(App)]
fn app() -> Html {
    let src_image_state = use_state(|| Rc::new(None));

    let onupload = {
        shadow_clone!(src_image_state);
        move |newdata| {
            src_image_state.set(newdata);
        }
    };

    html! {
        <div class="row justify-content-evenly">
            <div class="col-4">
                <h1>{"Satellite image"}</h1>
                <UploadPane {onupload} />
            </div>
            <div class="col-4">
                <h1>{"Segments"}</h1>
                <SegmentsPane image_data={(*src_image_state).clone()} />
            </div>
        </div>
    }
}

#[autoprops_component(SegmentsPane)]
fn segments_pane(image_data: Rc<Option<FileDetails>>) -> Html {
    let mask_image: UseStateHandle<Rc<Option<FileDetails>>> = use_state(|| Rc::new(None));

    let fallback = html!(
        <h1>{"Processing image..."} <span class="spinner-border text-success"></span></h1>
    );

    html!(
        <Suspense {fallback}>
            <SegmentsInnerPane src_image={image_data} />
        </Suspense>
    )
}

#[derive(Properties, PartialEq)]
struct SegmentsInnerPaneProps {
    src_image: Rc<Option<FileDetails>>,
}

#[function_component(SegmentsInnerPane)]
fn segments_inner_pane(props: &SegmentsInnerPaneProps) -> HtmlResult {
    let res = use_future_with(props.src_image.clone(), |deps| async move {
        if deps.is_none() {
            return None;
        }
        let FileDetails {
            file_name,
            file_type,
            data,
        } = (**deps).clone().unwrap();
        let client = reqwest::Client::new();
        let body = reqwest::multipart::Form::new().part(
            "f[]",
            reqwest::multipart::Part::bytes(data)
                .file_name(file_name)
                .mime_str(&file_type)
                .unwrap(),
        );
        let reqwest = client
            .post(format!("{}/segment", env!("SERVER_URL")))
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

        Some(result)
    })?;

    let answer = match *res {
        Some(ref res) => match res {
            Ok(file) => html! {
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
            },
            Err(why) => html!(
                <div class="alert alert-danger">
                    {"Could not fetch answer: "}{why}
                </div>
            ),
        },
        None => html!({ "No image uploaded yet..." }),
    };

    Ok(answer)
}

#[autoprops_component(UploadPane)]
fn upload_pane(#[prop_or_default] onupload: Callback<Rc<Option<FileDetails>>>) -> Html {
    let src_image_state = use_state(|| Rc::new(None));
    let readers = use_map(HashMap::new());

    let on_complete_read = {
        shadow_clone!(src_image_state, readers, onupload);
        move |file_name, file_type, data| {
            readers.remove(&file_name);

            log::info!("Finished reading {file_name}");
            let src_img = Rc::new(Some(FileDetails {
                file_name,
                file_type,
                data,
            }));

            src_image_state.set(src_img.clone());

            onupload.emit(src_img);
        }
    };

    let onupload = {
        shadow_clone!(src_image_state, readers);
        move |e: Event| {
            let input: HtmlInputElement = e.target_unchecked_into();
            let files = input.files();
            let files = match files {
                Some(f) => f,
                None => return,
            };
            let files = js_sys::try_iter(&files)
                .unwrap()
                .unwrap()
                .map(|v| web_sys::File::from(v.unwrap()))
                .map(File::from)
                .collect::<Vec<_>>();

            log::info!("New image: {files:?}");
            for file in files.into_iter() {
                let file_name = file.name();
                let file_type = file.raw_mime_type();

                let task = {
                    let file_name = file_name.clone();

                    gloo::file::callbacks::read_as_bytes(&file, {
                        shadow_clone!(on_complete_read);
                        move |res| {
                            on_complete_read(
                                file_name,
                                file_type,
                                res.expect("Failed to read file."),
                            )
                        }
                    })
                };
                readers.insert(file_name, task);
            }
        }
    };

    html!(
        <>
        {
            if let Some(file) = (*src_image_state).borrow() {
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
            onchange={onupload}
        />
        </>
    )
}

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<App>::new().render();
}
