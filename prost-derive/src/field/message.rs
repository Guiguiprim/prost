use anyhow::{bail, Error};
use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use syn::{Ident, Meta, Path};

use crate::field::{
    ident_attr, path_attr, set_bool, set_option, tag_attr, word_attr, Label, TyWithEncoding,
};

#[derive(Clone, Debug)]
pub struct MessageTy;

#[derive(Clone)]
pub struct Field {
    pub label: Label,
    pub tag: u32,
    pub ty: TyWithEncoding<MessageTy>,
}

impl Field {
    pub fn new(attrs: &[Meta], inferred_tag: Option<u32>) -> Result<Option<Field>, Error> {
        let mut message = false;
        let mut label = None;
        let mut tag = None;
        let mut boxed = false;
        let mut encoding_ty = None;
        let mut encoding_module = None;

        let mut unknown_attrs = Vec::new();

        for attr in attrs {
            if word_attr("message", attr) {
                set_bool(&mut message, "duplicate message attribute")?;
            } else if word_attr("boxed", attr) {
                set_bool(&mut boxed, "duplicate boxed attribute")?;
            } else if let Some(t) = tag_attr(attr)? {
                set_option(&mut tag, t, "duplicate tag attributes")?;
            } else if let Some(l) = Label::from_attr(attr) {
                set_option(&mut label, l, "duplicate label attributes")?;
            } else if let Some(ty) = ident_attr("encoding", attr)? {
                set_option(&mut encoding_ty, ty, "duplicate encoding attributes")?;
            } else if let Some(module) = path_attr("encoding_module", attr)? {
                set_option(
                    &mut encoding_module,
                    module,
                    "duplicate encoding_module attributes",
                )?;
            } else {
                unknown_attrs.push(attr);
            }
        }

        if !message {
            return Ok(None);
        }

        if !unknown_attrs.is_empty() {
            bail!(
                "unknown attribute(s) for message field: #[prost({})]",
                quote!(#(#unknown_attrs),*)
            );
        }

        let tag = match tag.or(inferred_tag) {
            Some(tag) => tag,
            None => bail!("message field is missing a tag attribute"),
        };

        Ok(Some(Field {
            label: label.unwrap_or(Label::Optional),
            tag,
            ty: TyWithEncoding::try_message_from(encoding_ty, encoding_module)?,
        }))
    }

    pub fn new_oneof(attrs: &[Meta]) -> Result<Option<Field>, Error> {
        if let Some(mut field) = Field::new(attrs, None)? {
            if let Some(attr) = attrs.iter().find(|attr| Label::from_attr(attr).is_some()) {
                bail!(
                    "invalid attribute for oneof field: {}",
                    attr.path().into_token_stream()
                );
            }
            field.label = Label::Required;
            Ok(Some(field))
        } else {
            Ok(None)
        }
    }

    pub fn encode(&self, prost_path: &Path, ident: TokenStream) -> TokenStream {
        let tag = self.tag;
        let encoding_ty = self.ty.encoding_ty(prost_path);
        match self.label {
            Label::Optional => quote! {
                if let Some(ref msg) = #ident {
                    #encoding_ty::encode(#tag, msg, buf);
                }
            },
            Label::Required => quote! {
                #encoding_ty::encode(#tag, &#ident, buf);
            },
            Label::Repeated => quote! {
                for msg in &#ident {
                    #encoding_ty::encode(#tag, msg, buf);
                }
            },
        }
    }

    pub fn merge(&self, prost_path: &Path, ident: TokenStream) -> TokenStream {
        let encoding_ty = self.ty.encoding_ty(prost_path);
        match self.label {
            Label::Optional => quote! {
                #encoding_ty::merge(wire_type,
                                                 #ident.get_or_insert_with(::core::default::Default::default),
                                                 buf,
                                                 ctx)
            },
            Label::Required => quote! {
                #encoding_ty::merge(wire_type, #ident, buf, ctx)
            },
            Label::Repeated => quote! {
                #encoding_ty::merge_repeated(wire_type, #ident, buf, ctx)
            },
        }
    }

    pub fn encoded_len(&self, prost_path: &Path, ident: TokenStream) -> TokenStream {
        let tag = self.tag;
        let encoding_ty = self.ty.encoding_ty(prost_path);
        match self.label {
            Label::Optional => quote! {
                #ident.as_ref().map_or(0, |msg| #encoding_ty::encoded_len(#tag, msg))
            },
            Label::Required => quote! {
                #encoding_ty::encoded_len(#tag, &#ident)
            },
            Label::Repeated => quote! {
                #encoding_ty::encoded_len_repeated(#tag, &#ident)
            },
        }
    }

    pub fn clear(&self, ident: TokenStream) -> TokenStream {
        match self.label {
            Label::Optional => quote!(#ident = ::core::option::Option::None),
            Label::Required => quote!(#ident.clear()),
            Label::Repeated => quote!(#ident.clear()),
        }
    }
}

impl TyWithEncoding<MessageTy> {
    pub fn try_message_from(
        encoding_ty: Option<Ident>,
        encoding_module: Option<Path>,
    ) -> Result<Self, Error> {
        if encoding_module.is_some() && encoding_ty.is_none() {
            bail!("encoding_module attribute can only be applied in pair with encoding attribute");
        }
        match encoding_ty {
            Some(encoding_ty) => Ok(TyWithEncoding {
                ty: MessageTy,
                encoding_ty,
                encoding_module,
            }),
            None => Ok(TyWithEncoding {
                ty: MessageTy,
                encoding_ty: Ident::new("MessageEncoding", Span::call_site()),
                encoding_module: None,
            }),
        }
    }
}
