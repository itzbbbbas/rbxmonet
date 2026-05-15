use std::collections::{BTreeMap, HashMap};

use dyn_fmt::AsStrFormatExt;
use nestify::nest;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::{fs, io::AsyncWriteExt};
use toml_edit::Array;

use crate::utils::{deserialize_regex_vec, serialize_regex_vec};
use crate::{
    Result, get_toml_value,
    ui::diffs::{DiffChange, ProductDiff, ProductDiffs},
};

nest! {
    #[derive(Default, Debug, Clone, Serialize, Deserialize)]*
    #[serde(rename_all = "snake_case")]*
    pub struct VCSProducts {
        pub metadata: pub struct Metadata {
            #[serde(alias = "universe-id")]
            pub universe_id: u64,
            #[serde(alias = "discount-prefix")]
            pub discount_prefix: Option<String>,
            #[serde(default, alias = "name-filters", deserialize_with = "deserialize_regex_vec", serialize_with = "serialize_regex_vec")]
            pub name_filters: Option<Vec<Regex>>,
        },

        #[serde(default)]
        pub codegen: pub struct CodegenConfig {
            #[serde(default)]
            pub output: Option<String>,
            #[serde(default)]
            pub style: CodegenStyle,
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub typescript: bool,
            #[serde(default)]
            pub paths: HashMap<String, String>,
            #[serde(default)]
            pub extra: HashMap<String, u64>,
        },

        #[serde(default)]
        pub passes: HashMap<String, pub struct Product {
            pub id: Option<u64>,
            pub name: String,
            pub prefix: Option<String>,
            pub description: Option<String>,
            pub active: bool,
            pub discount: Option<u8>,
            pub price: i64,
            #[serde(alias = "regional-pricing")]
            pub regional_pricing: Option<bool>,
            #[serde(default)]
            pub icon: Option<String>,
            #[serde(default)]
            pub path: Option<String>,
        }>,

        #[serde(default)]
        pub products: HashMap<String, Product>,

        #[serde(default)]
        pub badges: HashMap<String, Product>,
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodegenStyle {
    #[default]
    Flat,
    Nested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProductType {
    Pass,
    DevProduct,
    Badge,
}
pub enum MultiProduct {
    Pass(Product),
    DevProduct(Product),
    Badge(Product),
}

macro_rules! check_diff {
    ($diffs:expr, $field:expr, $old:expr, $new:expr, $variant:ident) => {
        if $old != $new {
            $diffs.push(DiffChange::Changed(ProductDiff::$variant(
                $old.clone(),
                $new.clone(),
            )));
        } else {
            $diffs.push(DiffChange::Unchanged(ProductDiff::$variant(
                $old.clone(),
                $new.clone(),
            )));
        }
    };
}

impl VCSProducts {
    pub async fn get_products() -> Result<Self> {
        let file_data = fs::read("rbxmonet.toml").await?;
        let text = std::str::from_utf8(&file_data)?;
        let doc: toml_edit::DocumentMut = text.parse()?;
        if doc
            .get("metadata")
            .and_then(|m| m.as_table())
            .and_then(|t| t.get("luau-file"))
            .is_some()
        {
            return Err("[metadata] luau-file is no longer supported \u{2014} move to [codegen] output (rbxmonet >= 0.1.21)".into());
        }
        if doc.get("gamepasses").is_some() {
            return Err("[gamepasses.*] is no longer supported \u{2014} rename to [passes.*] (rbxmonet >= 0.1.24)".into());
        }
        let mut products: VCSProducts = toml::from_str(text)?;
        normalize_slug_keys(&mut products.passes);
        normalize_slug_keys(&mut products.products);
        normalize_slug_keys(&mut products.badges);
        Ok(products)
    }

    pub async fn save_products(&self) -> Result<()> {
        let mut toml_products: toml_edit::DocumentMut;

        if let Ok(data) = fs::read("rbxmonet.toml").await {
            let document_string = String::from_utf8(data.clone())?;
            toml_products = document_string.parse()?;
        } else {
            toml_products = toml_edit::DocumentMut::new();
        }

        let mut metadata = get_toml_value!(toml_products, "metadata");
        let mut passes = get_toml_value!(toml_products, "passes");
        let mut products = get_toml_value!(toml_products, "products");
        let mut badges = get_toml_value!(toml_products, "badges");

        if let Some(discount_prefix) = &self.metadata.discount_prefix {
            metadata["discount_prefix"] = toml_edit::value(discount_prefix.clone());
        } else {
            metadata.remove("discount_prefix");
        }
        metadata.remove("discount-prefix");

        metadata["universe_id"] = toml_edit::value(self.metadata.universe_id as i64);
        metadata.remove("universe-id");
        metadata.remove("luau-file");

        let filters = self
            .metadata
            .name_filters
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|x| x.as_str().to_string())
            .collect::<Vec<_>>();

        metadata["name_filters"] = toml_edit::value(Array::from_iter(filters.iter()));
        metadata.remove("name-filters");

        passes.retain(|k, _| self.passes.contains_key(k));
        products.retain(|k, _| self.products.contains_key(k));
        badges.retain(|k, _| self.badges.contains_key(k));

        for pass in &self.passes {
            passes[&pass.0] = pass.1.into();
        }
        for product in &self.products {
            products[&product.0] = product.1.into();
        }
        for badge in &self.badges {
            badges[&badge.0] = badge.1.into();
        }

        toml_products.remove("subscriptions");
        toml_products.remove("gamepasses");
        toml_products["metadata"] = toml_edit::Item::Table(metadata);
        toml_products["passes"] = toml_edit::Item::Table(passes);
        toml_products["products"] = toml_edit::Item::Table(products);
        toml_products["badges"] = toml_edit::Item::Table(badges);

        fs::write("rbxmonet.toml", toml_products.to_string()).await?;
        Ok(())
    }

    pub async fn serialize_luau(&self) -> Result<()> {
        let products_lua_file = match self.codegen.output.clone() {
            Some(file) => file,
            None => {
                log::info!("codegen.output not set \u{2014} skipping luau generation");
                return Ok(());
            }
        };

        let discount_prefix = self
            .metadata
            .discount_prefix
            .clone()
            .unwrap_or_else(|| "💲{}% OFF💲 ".to_string());

        let format_display_name = |product: &Product| -> String {
            if product.has_discount() {
                let prefix =
                    discount_prefix.replace("{}", &product.discount.unwrap_or(0).to_string());
                format!("{}{}", prefix, product.name)
            } else if let Some(p) = &product.prefix {
                format!("{} {}", p, product.name)
            } else {
                product.name.clone()
            }
        };

        let resolve_section = |key: &str, default: &str| -> String {
            self.codegen
                .paths
                .get(key)
                .cloned()
                .unwrap_or_else(|| default.to_string())
        };

        let mut tree: CodegenTree = BTreeMap::new();

        let insert_product = |tree: &mut CodegenTree,
                               section_path: &str,
                               slug: &str,
                               p: &Product,
                               section: SectionKind| {
            let path = p.path.as_deref().unwrap_or(section_path);
            let leaf = LeafKind::Rich {
                section,
                id: p.id.unwrap_or(0),
                price: p.get_price() as i64,
                name: format_display_name(p),
                description: p.description.clone().unwrap_or_default(),
            };
            insert_into_tree(tree, &split_path(path), slug, leaf);
        };

        let pass_path = resolve_section("passes", "passes");
        let prod_path = resolve_section("products", "products");
        let badge_path = resolve_section("badges", "badges");

        for (slug, p) in &self.passes {
            insert_product(&mut tree, &pass_path, slug, p, SectionKind::Pass);
        }
        for (slug, p) in &self.products {
            insert_product(&mut tree, &prod_path, slug, p, SectionKind::Product);
        }
        for (slug, p) in &self.badges {
            insert_product(&mut tree, &badge_path, slug, p, SectionKind::Badge);
        }

        for (full_key, id) in &self.codegen.extra {
            let (path, leaf_key) = match full_key.rfind('.') {
                Some(i) => (&full_key[..i], &full_key[i + 1..]),
                None => ("", full_key.as_str()),
            };
            let segments = if path.is_empty() {
                Vec::new()
            } else {
                split_path(path)
            };
            insert_into_tree(&mut tree, &segments, leaf_key, LeafKind::IdOnly(*id));
        }

        let mut contents = String::new();
        contents += "-- This file is automatically generated by rbxmonet. Do not edit this file directly.\n";
        contents += "export type Kind = \"Pass\" | \"Product\" | \"Badge\"\n";
        contents += "export type Pass = { id: number, price: number, kind: Kind, name: string, description: string }\n";
        contents += "export type Product = { id: number, price: number, kind: Kind, name: string, description: string }\n";
        contents += "export type Badge = { id: number, price: number, kind: Kind, name: string, description: string }\n\n";

        match self.codegen.style {
            CodegenStyle::Flat => render_flat(&mut contents, &tree),
            CodegenStyle::Nested => render_nested(&mut contents, &tree),
        }

        let mut file = fs::File::create(&products_lua_file).await?;
        file.write_all(contents.as_bytes()).await?;

        if self.codegen.typescript {
            let dts_path = derive_dts_path(&products_lua_file);
            let var_name = derive_module_name(&products_lua_file);
            let mut dts = String::new();
            dts += "// This file is automatically generated by rbxmonet. Do not edit this file directly.\n";
            dts += "export type Kind = \"Pass\" | \"Product\" | \"Badge\";\n";
            dts += "export interface Pass { id: number; price: number; kind: \"Pass\"; name: string; description: string }\n";
            dts += "export interface Product { id: number; price: number; kind: \"Product\"; name: string; description: string }\n";
            dts += "export interface Badge { id: number; price: number; kind: \"Badge\"; name: string; description: string }\n\n";
            match self.codegen.style {
                CodegenStyle::Flat => render_flat_dts(&mut dts, &tree, &var_name),
                CodegenStyle::Nested => render_nested_dts(&mut dts, &tree, &var_name),
            }
            let mut dts_file = fs::File::create(&dts_path).await?;
            dts_file.write_all(dts.as_bytes()).await?;
        }

        Ok(())
    }
}

fn derive_dts_path(luau_path: &str) -> String {
    if let Some(stripped) = luau_path.strip_suffix(".luau") {
        format!("{}.d.ts", stripped)
    } else if let Some(stripped) = luau_path.strip_suffix(".lua") {
        format!("{}.d.ts", stripped)
    } else {
        format!("{}.d.ts", luau_path)
    }
}

fn derive_module_name(luau_path: &str) -> String {
    let stem = std::path::Path::new(luau_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("monets");
    if is_simple_lua_ident(stem) {
        stem.to_string()
    } else {
        let sanitized: String = stem
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
            .collect();
        if sanitized.is_empty() || !sanitized.chars().next().unwrap().is_ascii_alphabetic() {
            format!("_{}", sanitized)
        } else {
            sanitized
        }
    }
}

fn render_nested(out: &mut String, tree: &CodegenTree) {
    out.push_str("return {\n");
    let mut entries: Vec<(&String, &CodegenNode)> = tree.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let len = entries.len();
    for (i, (key, node)) in entries.iter().enumerate() {
        out.push_str(&format!("\t{} = ", format_lua_key(key)));
        render_node(out, node, 1);
        if i + 1 != len {
            out.push_str(",\n\n");
        } else {
            out.push_str("\n");
        }
    }
    out.push_str("}");
}

fn render_flat(out: &mut String, tree: &CodegenTree) {
    let mut leaves: Vec<(String, &LeafKind)> = Vec::new();
    collect_flat_leaves(tree, &mut Vec::new(), &mut leaves);
    leaves.sort_by(|a, b| a.0.cmp(&b.0));

    out.push_str("return {\n");
    let len = leaves.len();
    for (i, (path, leaf)) in leaves.iter().enumerate() {
        out.push_str(&format!("\t[{:?}] = ", path));
        render_leaf(out, leaf);
        if i + 1 != len {
            out.push_str(",\n");
        } else {
            out.push_str("\n");
        }
    }
    out.push_str("}");
}

fn collect_flat_leaves<'a>(
    tree: &'a CodegenTree,
    path: &mut Vec<String>,
    out: &mut Vec<(String, &'a LeafKind)>,
) {
    for (key, node) in tree {
        path.push(key.clone());
        match node {
            CodegenNode::Leaf(kind) => out.push((path.join("."), kind)),
            CodegenNode::Branch(children) => collect_flat_leaves(children, path, out),
        }
        path.pop();
    }
}

fn render_leaf(out: &mut String, leaf: &LeafKind) {
    match leaf {
        LeafKind::Rich {
            section,
            id,
            price,
            name,
            description,
        } => {
            out.push_str(&format!(
                "{{ id = {}, price = {}, kind = {:?} :: Kind, name = {:?}, description = {:?} }}",
                id,
                price,
                section.ts_name(),
                name,
                description
            ));
        }
        LeafKind::IdOnly(id) => {
            out.push_str(&format!("{{ id = {} }}", id));
        }
    }
}

fn leaf_ts_type(leaf: &LeafKind) -> &'static str {
    match leaf {
        LeafKind::IdOnly(_) => "{ id: number }",
        LeafKind::Rich { section, .. } => section.ts_name(),
    }
}

fn render_flat_dts(out: &mut String, tree: &CodegenTree, var_name: &str) {
    let mut leaves: Vec<(String, &LeafKind)> = Vec::new();
    collect_flat_leaves(tree, &mut Vec::new(), &mut leaves);
    leaves.sort_by(|a, b| a.0.cmp(&b.0));

    out.push_str(&format!("declare const {}: {{\n", var_name));
    for (path, leaf) in &leaves {
        out.push_str(&format!("\t{:?}: {};\n", path, leaf_ts_type(leaf)));
    }
    out.push_str("};\n");
    out.push_str(&format!("export default {};\n", var_name));
}

fn render_nested_dts(out: &mut String, tree: &CodegenTree, var_name: &str) {
    out.push_str(&format!("declare const {}: ", var_name));
    render_ts_branch(out, tree, 0);
    out.push_str(";\n");
    out.push_str(&format!("export default {};\n", var_name));
}

fn render_ts_branch(out: &mut String, tree: &CodegenTree, depth: usize) {
    let indent = "\t".repeat(depth);
    let inner = "\t".repeat(depth + 1);
    out.push_str("{\n");
    let mut entries: Vec<(&String, &CodegenNode)> = tree.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    for (key, node) in &entries {
        let key_repr = if is_simple_lua_ident(key) {
            key.to_string()
        } else {
            format!("{:?}", key)
        };
        match node {
            CodegenNode::Leaf(kind) => {
                out.push_str(&format!("{}{}: {};\n", inner, key_repr, leaf_ts_type(kind)));
            }
            CodegenNode::Branch(children) => {
                out.push_str(&format!("{}{}: ", inner, key_repr));
                render_ts_branch(out, children, depth + 1);
                out.push_str(";\n");
            }
        }
    }
    out.push_str(&format!("{}}}", indent));
}

// ---------------------------------------------------------------------------
// Codegen tree: borrowed structure from dev-bap/rbxsync, adapted to emit
// rich `{ id, price, name, description }` leaves for rbxmonet entries while
// also supporting `{ id = N }` leaves for `[codegen.extra]` injections.
// ---------------------------------------------------------------------------

type CodegenTree = BTreeMap<String, CodegenNode>;

enum CodegenNode {
    Leaf(LeafKind),
    Branch(BTreeMap<String, CodegenNode>),
}

#[derive(Clone, Copy)]
enum SectionKind {
    Pass,
    Product,
    Badge,
}

impl SectionKind {
    fn ts_name(self) -> &'static str {
        match self {
            SectionKind::Pass => "Pass",
            SectionKind::Product => "Product",
            SectionKind::Badge => "Badge",
        }
    }
}

#[derive(Clone)]
enum LeafKind {
    Rich {
        section: SectionKind,
        id: u64,
        price: i64,
        name: String,
        description: String,
    },
    IdOnly(u64),
}

fn normalize_slug_keys(map: &mut HashMap<String, Product>) {
    let renames: Vec<(String, String)> = map
        .keys()
        .filter(|k| k.contains('-'))
        .map(|k| (k.clone(), k.replace('-', "_")))
        .collect();
    for (old, new) in renames {
        if map.contains_key(&new) {
            log::warn!(
                "slug normalization: '{}' would collide with existing '{}', skipping rename",
                old,
                new
            );
            continue;
        }
        if let Some(v) = map.remove(&old) {
            map.insert(new, v);
        }
    }
}

fn split_path(path: &str) -> Vec<&str> {
    path.split('.').filter(|s| !s.is_empty()).collect()
}

fn insert_into_tree(tree: &mut CodegenTree, segments: &[&str], key: &str, leaf: LeafKind) {
    if segments.is_empty() {
        if tree.contains_key(key) {
            log::warn!("codegen: overwriting key '{}'", key);
        }
        tree.insert(key.to_string(), CodegenNode::Leaf(leaf));
        return;
    }

    let head = segments[0].to_string();
    let entry = tree
        .entry(head.clone())
        .or_insert_with(|| CodegenNode::Branch(BTreeMap::new()));
    match entry {
        CodegenNode::Branch(children) => {
            insert_into_tree(children, &segments[1..], key, leaf);
        }
        CodegenNode::Leaf(_) => {
            log::warn!(
                "codegen: path segment '{}' is both a leaf and a branch; promoting to branch",
                head
            );
            let mut children = BTreeMap::new();
            insert_into_tree(&mut children, &segments[1..], key, leaf);
            *entry = CodegenNode::Branch(children);
        }
    }
}

fn is_simple_lua_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn format_lua_key(key: &str) -> String {
    if is_simple_lua_ident(key) {
        key.to_string()
    } else {
        format!("[{:?}]", key)
    }
}

fn render_node(out: &mut String, node: &CodegenNode, depth: usize) {
    let indent = "\t".repeat(depth);
    match node {
        CodegenNode::Leaf(LeafKind::Rich {
            section,
            id,
            price,
            name,
            description,
        }) => {
            out.push_str(&format!(
                "{{ id = {}, price = {}, kind = {:?} :: Kind, name = {:?}, description = {:?} }}",
                id,
                price,
                section.ts_name(),
                name,
                description
            ));
        }
        CodegenNode::Leaf(LeafKind::IdOnly(id)) => {
            out.push_str(&format!("{{ id = {} }}", id));
        }
        CodegenNode::Branch(children) => {
            out.push_str("{\n");
            let mut entries: Vec<(&String, &CodegenNode)> = children.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let len = entries.len();
            for (i, (key, child)) in entries.iter().enumerate() {
                out.push_str(&format!("{}\t{} = ", indent, format_lua_key(key)));
                render_node(out, child, depth + 1);
                if i + 1 != len {
                    out.push_str(",\n");
                } else {
                    out.push_str("\n");
                }
            }
            out.push_str(&format!("{}}}", indent));
        }
    }
}

impl Product {
    pub fn has_discount(&self) -> bool {
        if let Some(discount) = self.discount
            && discount > 0
        {
            true
        } else {
            false
        }
    }

    pub fn get_price(&self) -> u64 {
        if let Some(discount) = self.discount
            && discount > 0
        {
            (self.price as f64 * (1.0 - (discount as f64 / 100.0))).floor() as u64
        } else {
            self.price as u64
        }
    }

    pub fn get_title(&self) -> String {
        if self.has_discount() {
            return self.name.clone();
        }

        if let Some(prefix) = &self.prefix {
            format!("{} {}", prefix, self.name)
        } else {
            self.name.clone()
        }
    }

    pub fn diff(&self, other: &Self, metadata: Option<&Metadata>) -> Option<ProductDiffs> {
        let mut diffs = vec![] as Vec<DiffChange>;

        let title = if let Some(metadata) = metadata {
            if let (Some(discount), Some(prefix)) = (self.discount, &metadata.discount_prefix) {
                format!("{} {}", prefix.format(&[discount]), self.get_title())
            } else {
                self.get_title()
            }
        } else {
            self.get_title()
        };

        let active = self.active;
        let price = self.get_price();
        let description = self.description.clone().unwrap_or(String::default());

        check_diff!(diffs, Title, other.name, title, Title);
        check_diff!(
            diffs,
            Description,
            other.description.clone().unwrap(),
            description,
            Description
        );
        check_diff!(diffs, Price, other.price as u64, price, Price);
        check_diff!(
            diffs,
            RegionalPricing,
            other.regional_pricing.unwrap_or(false),
            self.regional_pricing.unwrap_or(false),
            RegionalPricing
        );
        check_diff!(diffs, Active, other.active, active, Active);

        let has_diffs = diffs.iter().any(|d| match d {
            DiffChange::Changed(_) => true,
            _ => false,
        });

        if has_diffs {
            Some(ProductDiffs {
                name: self.name.clone(),
                id: self.id.unwrap_or(0) as u64,
                diffs,
            })
        } else {
            None
        }
    }
}

impl From<&Product> for toml_edit::Item {
    fn from(prod: &Product) -> Self {
        let mut table = toml_edit::Table::new();

        if let Some(id) = prod.id {
            table["id"] = toml_edit::value(id as i64);
        }

        if let Some(prefix) = &prod.prefix {
            table["prefix"] = toml_edit::value(prefix.clone());
        }

        table["name"] = toml_edit::value(&prod.name);

        if let Some(desc) = &prod.description {
            table["description"] = toml_edit::value(desc);
        }

        table["active"] = toml_edit::value(prod.active);

        if let Some(discount) = prod.discount {
            table["discount"] = toml_edit::value(discount as i64);
        }

        table["price"] = toml_edit::value(prod.price);

        if let Some(regional_pricing) = prod.regional_pricing {
            table["regional_pricing"] = toml_edit::value(regional_pricing);
        }

        if let Some(icon) = &prod.icon {
            table["icon"] = toml_edit::value(icon.clone());
        }

        if let Some(path) = &prod.path {
            table["path"] = toml_edit::value(path.clone());
        }

        toml_edit::Item::Table(table)
    }
}

