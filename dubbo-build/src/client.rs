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

use super::{generate_doc_comments, naive_snake_case, Attributes};
use crate::{Method, Service};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub const CODEC_PATH: &str = "triple::codec::prost::ProstCodec";

/// Generate service for client.
///
/// This takes some `Service` and will generate a `TokenStream` that contains
/// a public module with the generated client.
pub fn generate<T: Service>(
    service: &T,
    emit_package: bool,
    proto_path: &str,
    compile_well_known_types: bool,
    attributes: &Attributes,
) -> TokenStream {
    let service_ident = quote::format_ident!("{}Client", service.name());
    let client_mod = quote::format_ident!("{}_client", naive_snake_case(service.name()));
    let methods = generate_methods(service, emit_package, proto_path, compile_well_known_types);

    let service_doc = generate_doc_comments(service.comment());

    let package = if emit_package { service.package() } else { "" };
    let path = format!(
        "{}{}{}",
        package,
        if package.is_empty() { "" } else { "." },
        service.identifier()
    );

    let mod_attributes = attributes.for_mod(package);
    let struct_attributes = attributes.for_struct(&path);

    quote! {
        /// Generated client implementations.
        #(#mod_attributes)*
        pub mod #client_mod {
            #![allow(
                unused_variables,
                dead_code,
                missing_docs,
                // will trigger if compression is disabled
                clippy::let_unit_value,
            )]
            use triple::client::TripleClient;
            use triple::invocation::*;
            use triple::server::Decoding;

            #service_doc
            #(#struct_attributes)*
            #[derive(Debug, Clone)]
            pub struct #service_ident {
                inner: TripleClient,
                uri: String,
            }

            impl #service_ident {
                pub fn new() -> Self {
                    Self {
                        inner: TripleClient::new(),
                        uri: "".to_string(),
                    }
                }

                pub fn with_uri(mut self, uri: String) -> Self {
                    self.uri = uri.clone();
                    self.inner = self.inner.with_host(uri);
                    self
                }

                #methods

            }
        }
    }
}

fn generate_methods<T: Service>(
    service: &T,
    emit_package: bool,
    proto_path: &str,
    compile_well_known_types: bool,
) -> TokenStream {
    let mut stream = TokenStream::new();
    let package = if emit_package { service.package() } else { "" };

    for method in service.methods() {
        let path = format!(
            "/{}{}{}/{}",
            package,
            if package.is_empty() { "" } else { "." },
            service.identifier(),
            method.identifier()
        );

        stream.extend(generate_doc_comments(method.comment()));

        let method = match (method.client_streaming(), method.server_streaming()) {
            (false, false) => generate_unary(&method, proto_path, compile_well_known_types, path),
            (false, true) => {
                generate_server_streaming(&method, proto_path, compile_well_known_types, path)
            }
            (true, false) => {
                generate_client_streaming(&method, proto_path, compile_well_known_types, path)
            }
            (true, true) => generate_streaming(&method, proto_path, compile_well_known_types, path),
        };

        stream.extend(method);
    }

    stream
}

fn generate_unary<T: Method>(
    method: &T,
    proto_path: &str,
    compile_well_known_types: bool,
    path: String,
) -> TokenStream {
    let codec_name = syn::parse_str::<syn::Path>(CODEC_PATH).unwrap();
    let ident = format_ident!("{}", method.name());
    let (request, response) = method.request_response_name(proto_path, compile_well_known_types);

    quote! {
        pub async fn #ident(
            &mut self,
            request: Request<#request>,
        ) -> Result<Response<#response>, triple::status::Status> {
           let codec = #codec_name::<#request, #response>::default();
           let path = http::uri::PathAndQuery::from_static(#path);
           self.inner
            .unary(
                request,
                codec,
                path,
            )
            .await
        }
    }
}

fn generate_server_streaming<T: Method>(
    method: &T,
    proto_path: &str,
    compile_well_known_types: bool,
    path: String,
) -> TokenStream {
    let codec_name = syn::parse_str::<syn::Path>(CODEC_PATH).unwrap();
    let ident = format_ident!("{}", method.name());

    let (request, response) = method.request_response_name(proto_path, compile_well_known_types);

    quote! {
        pub async fn #ident(
            &mut self,
            request: Request<#request>,
        ) -> Result<Response<Decoding<#response>>, triple::status::Status> {

            let codec = #codec_name::<#request, #response>::default();
            let path = http::uri::PathAndQuery::from_static(#path);
            self.inner.server_streaming(request, codec, path).await
        }
    }
}

fn generate_client_streaming<T: Method>(
    method: &T,
    proto_path: &str,
    compile_well_known_types: bool,
    path: String,
) -> TokenStream {
    let codec_name = syn::parse_str::<syn::Path>(CODEC_PATH).unwrap();
    let ident = format_ident!("{}", method.name());

    let (request, response) = method.request_response_name(proto_path, compile_well_known_types);

    quote! {
        pub async fn #ident(
            &mut self,
            request: impl IntoStreamingRequest<Message = #request>
        ) -> Result<Response<#response>, triple::status::Status> {
            let codec = #codec_name::<#request, #response>::default();
            let path = http::uri::PathAndQuery::from_static(#path);
            self.inner.client_streaming(request, codec, path).await
        }
    }
}

fn generate_streaming<T: Method>(
    method: &T,
    proto_path: &str,
    compile_well_known_types: bool,
    path: String,
) -> TokenStream {
    let codec_name = syn::parse_str::<syn::Path>(CODEC_PATH).unwrap();
    let ident = format_ident!("{}", method.name());

    let (request, response) = method.request_response_name(proto_path, compile_well_known_types);

    quote! {
        pub async fn #ident(
            &mut self,
            request: impl IntoStreamingRequest<Message = #request>
        ) -> Result<Response<Decoding<#response>>, triple::status::Status> {
            let codec = #codec_name::<#request, #response>::default();
            let path = http::uri::PathAndQuery::from_static(#path);
            self.inner.bidi_streaming(request, codec, path).await
        }
    }
}
