/*
 * Licensed to the Apache Software Foundation (ASF) under one or more
 * contributor license agreements.  See the NOTICE file distributed with
 * this work for additional information regarding copyright ownership.
 * The ASF licenses this file to You under the Apache License, Version 2.0
 * (the "License"); you may not use this file except in compliance with
 * the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use proc_macro2::TokenStream;
use prost_build::{Config, Method, ServiceGenerator};
use quote::ToTokens;
use std::path::{Path, PathBuf};

use crate::client;
use crate::server;
use crate::Attributes;

/// Simple `.proto` compiling. Use [`configure`] instead if you need more options.
///
/// The include directory will be the parent folder of the specified path.
/// The package name will be the filename without the extension.
pub fn compile_protos(proto: impl AsRef<Path>) -> std::io::Result<()> {
    let proto_path: &Path = proto.as_ref();

    // directory the main .proto file resides in
    let proto_dir = proto_path
        .parent()
        .expect("proto file should reside in a directory");

    self::configure().compile(&[proto_path], &[proto_dir])?;

    Ok(())
}

pub fn configure() -> Builder {
    Builder {
        build_client: true,
        build_server: true,
        proto_path: "super".to_string(),
        protoc_args: Vec::new(),
        compile_well_known_types: false,
        include_file: None,
        output_dir: None,
        server_attributes: Attributes::default(),
        client_attributes: Attributes::default(),
    }
}

pub struct Builder {
    build_client: bool,
    build_server: bool,
    proto_path: String,
    compile_well_known_types: bool,
    protoc_args: Vec<String>,
    include_file: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    server_attributes: Attributes,
    client_attributes: Attributes,
}

impl Builder {
    pub fn output_dir(mut self, output_dir: PathBuf) -> Self {
        self.output_dir = Some(output_dir);
        self
    }

    pub fn compile(
        self,
        protos: &[impl AsRef<Path>],
        includes: &[impl AsRef<Path>],
    ) -> std::io::Result<()> {
        self.compile_with_config(Config::new(), protos, includes)
    }

    pub fn compile_with_config(
        self,
        mut config: Config,
        protos: &[impl AsRef<Path>],
        includes: &[impl AsRef<Path>],
    ) -> std::io::Result<()> {
        let out_dir = if let Some(out_dir) = self.output_dir.as_ref() {
            out_dir.clone()
        } else {
            PathBuf::from(std::env::var("OUT_DIR").unwrap())
        };
        config.out_dir(out_dir);

        if self.compile_well_known_types {
            config.compile_well_known_types();
        }

        if let Some(path) = self.include_file.as_ref() {
            config.include_file(path);
        }

        for arg in self.protoc_args.iter() {
            config.protoc_arg(arg);
        }

        config.service_generator(Box::new(SvcGenerator::new(self)));
        config.compile_protos(protos, includes)?;

        Ok(())
    }
}

pub struct SvcGenerator {
    builder: Builder,
    clients: TokenStream,
    servers: TokenStream,
}

impl SvcGenerator {
    fn new(builder: Builder) -> Self {
        SvcGenerator {
            builder,
            clients: TokenStream::new(),
            servers: TokenStream::new(),
        }
    }
}

impl ServiceGenerator for SvcGenerator {
    fn generate(&mut self, service: prost_build::Service, _buf: &mut String) {
        let svc = DubboService::new(service);
        if self.builder.build_server {
            let server = server::generate(
                &svc,
                true,
                &self.builder.proto_path,
                self.builder.compile_well_known_types,
                &self.builder.server_attributes,
            );
            self.servers.extend(server);
        }

        if self.builder.build_client {
            let client = client::generate(
                &svc,
                true,
                &self.builder.proto_path,
                self.builder.compile_well_known_types,
                &self.builder.client_attributes,
            );
            self.clients.extend(client);
        }
    }

    fn finalize(&mut self, buf: &mut String) {
        if self.builder.build_client && !self.clients.is_empty() {
            let clients = &self.clients;

            let client_services = quote::quote! {
                #clients
            };

            let ast: syn::File = syn::parse2(client_services).expect("invalid tokenstream");
            let code = prettyplease::unparse(&ast);
            buf.push_str(&code);

            self.clients = TokenStream::default();
        }

        if self.builder.build_server && !self.servers.is_empty() {
            let servers = &self.servers;

            let server_services = quote::quote! {
                #servers
            };

            let ast: syn::File = syn::parse2(server_services).expect("invalid tokenstream");
            let code = prettyplease::unparse(&ast);
            buf.push_str(&code);

            self.servers = TokenStream::default();
        }
    }
}

pub struct DubboService {
    inner: prost_build::Service,
}

impl DubboService {
    fn new(inner: prost_build::Service) -> DubboService {
        Self { inner }
    }
}

impl super::Service for DubboService {
    type Comment = String;

    type Method = DubboMethod;

    fn name(&self) -> &str {
        &self.inner.name
    }

    fn package(&self) -> &str {
        &self.inner.package
    }

    fn identifier(&self) -> &str {
        &self.inner.proto_name
    }

    fn methods(&self) -> Vec<Self::Method> {
        let mut ms = Vec::new();
        for m in &self.inner.methods[..] {
            ms.push(DubboMethod::new(Method {
                name: m.name.clone(),
                proto_name: m.proto_name.clone(),
                comments: prost_build::Comments {
                    leading_detached: m.comments.leading_detached.clone(),
                    leading: m.comments.leading.clone(),
                    trailing: m.comments.trailing.clone(),
                },
                input_type: m.input_type.clone(),
                output_type: m.output_type.clone(),
                input_proto_type: m.input_proto_type.clone(),
                output_proto_type: m.output_proto_type.clone(),
                options: m.options.clone(),
                client_streaming: m.client_streaming,
                server_streaming: m.server_streaming,
            }))
        }

        ms
    }

    fn comment(&self) -> &[Self::Comment] {
        &self.inner.comments.leading[..]
    }
}

impl Clone for DubboService {
    fn clone(&self) -> Self {
        Self {
            inner: prost_build::Service {
                name: self.inner.name.clone(),
                proto_name: self.inner.proto_name.clone(),
                package: self.inner.package.clone(),
                methods: {
                    let mut ms = Vec::new();
                    for m in &self.inner.methods[..] {
                        ms.push(Method {
                            name: m.name.clone(),
                            proto_name: m.proto_name.clone(),
                            comments: prost_build::Comments {
                                leading_detached: m.comments.leading_detached.clone(),
                                leading: m.comments.leading.clone(),
                                trailing: m.comments.trailing.clone(),
                            },
                            input_type: m.input_type.clone(),
                            output_type: m.output_type.clone(),
                            input_proto_type: m.input_proto_type.clone(),
                            output_proto_type: m.output_proto_type.clone(),
                            options: m.options.clone(),
                            client_streaming: m.client_streaming,
                            server_streaming: m.server_streaming,
                        })
                    }

                    ms
                },
                comments: prost_build::Comments {
                    leading_detached: self.inner.comments.leading_detached.clone(),
                    leading: self.inner.comments.leading.clone(),
                    trailing: self.inner.comments.trailing.clone(),
                },
                options: self.inner.options.clone(),
            },
        }
    }
}

pub struct DubboMethod {
    inner: Method,
}

impl DubboMethod {
    fn new(m: Method) -> DubboMethod {
        Self { inner: m }
    }
}

impl super::Method for DubboMethod {
    type Comment = String;

    fn name(&self) -> &str {
        &self.inner.name
    }

    fn identifier(&self) -> &str {
        &self.inner.proto_name
    }

    fn codec_path(&self) -> &str {
        "triple::codec::serde_codec::SerdeCodec"
    }

    fn client_streaming(&self) -> bool {
        self.inner.client_streaming
    }

    fn server_streaming(&self) -> bool {
        self.inner.server_streaming
    }

    fn comment(&self) -> &[Self::Comment] {
        &self.inner.comments.leading[..]
    }

    fn request_response_name(
        &self,
        proto_path: &str,
        compile_well_known_types: bool,
    ) -> (TokenStream, TokenStream) {
        let convert_type = |proto_type: &str, rust_type: &str| -> TokenStream {
            if (is_google_type(proto_type) && !compile_well_known_types)
                || rust_type.starts_with("::")
                || NON_PATH_TYPE_ALLOWLIST.iter().any(|t| *t == rust_type)
            {
                rust_type.parse::<TokenStream>().unwrap()
            } else if rust_type.starts_with("crate::") {
                syn::parse_str::<syn::Path>(rust_type)
                    .unwrap()
                    .to_token_stream()
            } else {
                syn::parse_str::<syn::Path>(&format!("{}::{}", proto_path, rust_type))
                    .unwrap()
                    .to_token_stream()
            }
        };

        let req = convert_type(&self.inner.input_proto_type, &self.inner.input_type);
        let resp = convert_type(&self.inner.output_proto_type, &self.inner.output_type);

        (req, resp)
    }
}

impl Clone for DubboMethod {
    fn clone(&self) -> Self {
        DubboMethod::new(Method {
            name: self.inner.name.clone(),
            proto_name: self.inner.proto_name.clone(),
            comments: prost_build::Comments {
                leading_detached: self.inner.comments.leading_detached.clone(),
                leading: self.inner.comments.leading.clone(),
                trailing: self.inner.comments.trailing.clone(),
            },
            input_type: self.inner.input_type.clone(),
            output_type: self.inner.output_type.clone(),
            input_proto_type: self.inner.input_proto_type.clone(),
            output_proto_type: self.inner.output_proto_type.clone(),
            options: self.inner.options.clone(),
            client_streaming: self.inner.client_streaming,
            server_streaming: self.inner.server_streaming,
        })
    }
}

/// Non-path Rust types allowed for request/response types.
const NON_PATH_TYPE_ALLOWLIST: &[&str] = &["()"];

fn is_google_type(proto_type: &str) -> bool {
    proto_type.starts_with(".google.protobuf")
}
