//! El `manifest.json` de una release: qué descargar y con qué hash.
//!
//! Lo genera `tools/build-manifest.mjs` en el pipeline y es el único sitio del
//! que el instalador saca URLs. Nada se descarga sin un SHA-256 declarado aquí.

use serde::{Deserialize, Serialize};

/// Versión del formato que entiende este instalador.
pub const SCHEMA_VERSION: u32 = 1;

/// De dónde se lee el manifest de cada canal.
///
/// «stable» apunta a la última release publicada; «beta» a una etiqueta fija
/// que el pipeline mueve. La variable `KEIROST_MANIFEST_URL` las sustituye,
/// que es como se prueba un instalador contra artefactos propios sin publicar
/// nada.
pub const STABLE_URL: &str =
    "https://github.com/OpenFactu/destokp-program-and-setup/releases/latest/download/manifest.json";
pub const BETA_URL: &str =
    "https://github.com/OpenFactu/destokp-program-and-setup/releases/download/beta/manifest.json";

/// ¿Se puede descargar de aquí?
///
/// HTTPS siempre; HTTP sólo contra el propio equipo, que es como se prueba una
/// instalación con artefactos servidos en local.
fn es_origen_admisible(url: &str) -> bool {
    url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost")
        || url.starts_with("http://[::1]")
}

/// URL del manifest de un canal.
pub fn url_for_channel(channel: &str) -> String {
    if let Ok(url) = std::env::var("KEIROST_MANIFEST_URL") {
        if !url.trim().is_empty() {
            return url;
        }
    }
    match channel.trim().to_ascii_lowercase().as_str() {
        "beta" => BETA_URL.to_string(),
        _ => STABLE_URL.to_string(),
    }
}

/// URL del manifest de una versión concreta.
///
/// Cada publicación deja su propio `manifest.json` en su etiqueta, así que
/// instalar una versión pasada es pedir el suyo en vez del del canal. Hace
/// falta para dejar un equipo igual que otro y para volver atrás cuando una
/// versión nueva rompe algo.
pub fn url_for_version(version: &str) -> String {
    format!(
        "https://github.com/OpenFactu/destokp-program-and-setup/releases/download/keirost-v{}/manifest.json",
        version.trim().trim_start_matches('v')
    )
}

/// La URL que toca: la de la versión pedida, o la del canal si no se pide
/// ninguna.
pub fn url_para(channel: &str, version: Option<&str>) -> String {
    match version.map(str::trim).filter(|v| !v.is_empty()) {
        // `KEIROST_MANIFEST_URL` manda siempre: es como se prueban artefactos
        // sin publicar, y ahí no hay versiones que elegir.
        Some(_) if std::env::var("KEIROST_MANIFEST_URL").is_ok_and(|u| !u.trim().is_empty()) => {
            url_for_channel(channel)
        }
        Some(version) => url_for_version(version),
        None => url_for_channel(channel),
    }
}

/// Comprueba que el manifest descargado es de la versión que se pidió.
///
/// Que la etiqueta exista no garantiza lo que hay dentro. Instalar otra cosa en
/// silencio sería peor que fallar: el equipo acabaría con una versión que nadie
/// eligió y nadie sabría por qué.
pub fn comprobar_version(manifest: Manifest, esperada: Option<&str>) -> crate::Result<Manifest> {
    let Some(esperada) = esperada.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(manifest);
    };
    if manifest.keirost.version.trim() == esperada.trim_start_matches('v') {
        return Ok(manifest);
    }
    Err(crate::Error::VersionNotFound {
        version: esperada.to_string(),
        channel: manifest.channel.clone(),
    })
}

/// Prefijo de las etiquetas con las que se publica cada versión.
const ETIQUETA: &str = "keirost-v";

/// Versiones publicadas, de la más reciente a la más antigua.
///
/// Sale de las releases del repositorio: son la misma fuente de la que salen
/// los artefactos, así que lo que aparezca aquí se puede instalar seguro. Que
/// falle no es grave —se sigue pudiendo escribir la versión a mano— así que
/// quien llama decide qué hacer con el error.
pub fn published_versions(limite: usize) -> crate::Result<Vec<String>> {
    let url = format!(
        "https://api.github.com/repos/OpenFactu/destokp-program-and-setup/releases?per_page={}",
        limite.clamp(1, 100)
    );

    let cuerpo = ureq::get(&url)
        // GitHub rechaza las peticiones sin agente.
        .header("User-Agent", "keirost-setup")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|source| crate::Error::Download {
            url: url.clone(),
            source: Box::new(source),
        })?
        .into_body()
        .read_to_string()
        .map_err(|source| crate::Error::Download {
            url,
            source: Box::new(source),
        })?;

    Ok(versiones_de(&cuerpo))
}

/// Extrae las versiones de la respuesta de GitHub.
///
/// Se ignora todo lo que no sea una publicación de artefactos: el canal beta
/// usa una etiqueta fija («beta») que no nombra ninguna versión, y el mismo
/// repositorio publica también los instaladores con otro prefijo.
pub fn versiones_de(json: &str) -> Vec<String> {
    let Ok(releases) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return Vec::new();
    };
    releases
        .iter()
        .filter_map(|r| r.get("tag_name")?.as_str())
        .filter_map(|tag| tag.strip_prefix(ETIQUETA))
        .map(str::to_string)
        .collect()
}

/// Descarga el manifest del canal indicado (la última versión publicada).
pub fn fetch(channel: &str) -> crate::Result<Manifest> {
    fetch_version(channel, None)
}

/// Descarga el manifest de una versión concreta, o el del canal si no se indica.
pub fn fetch_version(channel: &str, version: Option<&str>) -> crate::Result<Manifest> {
    let url = url_para(channel, version);
    let body = ureq::get(&url)
        .call()
        .map_err(|source| crate::Error::Download {
            url: url.clone(),
            source: Box::new(source),
        })?
        .into_body()
        .read_to_string()
        .map_err(|source| crate::Error::Download {
            url,
            source: Box::new(source),
        })?;

    comprobar_version(Manifest::parse(&body)?, version)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub channel: String,
    pub keirost: Release,
    pub artifacts: Artifacts,
    /// Artefactos de los componentes opcionales. Una release puede no
    /// publicarlos: en ese caso el instalador avisa y sigue sin ellos, en vez
    /// de fallar.
    #[serde(default)]
    pub extras: Option<Extras>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Extras {
    #[serde(default)]
    pub ollama: Option<Artifact>,
    #[serde(default)]
    pub prometheus: Option<Artifact>,
    #[serde(default)]
    pub grafana: Option<Artifact>,
    #[serde(default, rename = "windowsExporter")]
    pub windows_exporter: Option<Artifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    #[serde(rename = "platformRef")]
    pub platform_ref: Option<String>,
    #[serde(rename = "releasedAt")]
    pub released_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifacts {
    pub server: Artifact,
    pub web: Artifact,
    pub chromium: Artifact,
    pub node: Artifact,
    pub postgres: Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub file: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

impl Manifest {
    pub fn parse(json: &str) -> crate::Result<Self> {
        let manifest: Manifest =
            serde_json::from_str(json).map_err(|e| crate::Error::Manifest(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> crate::Result<()> {
        if self.schema != SCHEMA_VERSION {
            return Err(crate::Error::ManifestVersion {
                found: self.schema,
                expected: SCHEMA_VERSION,
            });
        }
        for (nombre, artefacto) in self.all() {
            if artefacto.sha256.len() != 64
                || !artefacto.sha256.chars().all(|c| c.is_ascii_hexdigit())
            {
                return Err(crate::Error::Manifest(format!(
                    "el artefacto «{nombre}» no trae un SHA-256 válido"
                )));
            }
            if !es_origen_admisible(&artefacto.url) {
                // Descargar por HTTP dejaría el instalador a merced de
                // cualquiera en la red del cliente. Se admite el bucle local
                // porque ahí no hay red que interceptar, y es lo que permite
                // probar una instalación completa contra artefactos propios
                // antes de publicarlos.
                return Err(crate::Error::Manifest(format!(
                    "el artefacto «{nombre}» no se descarga por HTTPS"
                )));
            }
        }
        Ok(())
    }

    /// Todos los artefactos con su nombre lógico.
    pub fn all(&self) -> Vec<(&'static str, &Artifact)> {
        vec![
            ("server", &self.artifacts.server),
            ("web", &self.artifacts.web),
            ("chromium", &self.artifacts.chromium),
            ("node", &self.artifacts.node),
            ("postgres", &self.artifacts.postgres),
        ]
    }

    /// Lo que hay que descargar para un perfil concreto.
    ///
    /// El perfil «sólo escritorio» no baja ni servidor ni base de datos: son
    /// varios cientos de megas que no va a usar.
    pub fn required_for(
        &self,
        profile: crate::settings::Profile,
    ) -> Vec<(&'static str, &Artifact)> {
        if !profile.installs_server() {
            return Vec::new();
        }
        self.all()
    }

    /// Tamaño total de la descarga, para poder avisar antes de empezar.
    pub fn total_size(&self, profile: crate::settings::Profile) -> u64 {
        self.required_for(profile).iter().map(|(_, a)| a.size).sum()
    }

    /// Artefactos de los extras que se han pedido y que esta release publica.
    ///
    /// Lo que se pide pero no existe se devuelve aparte, para poder decir
    /// exactamente qué se va a quedar sin instalar en vez de fallar entero.
    pub fn extras_for(
        &self,
        optionals: &crate::settings::Optionals,
    ) -> (Vec<(&'static str, &Artifact)>, Vec<&'static str>) {
        let mut disponibles = Vec::new();
        let mut ausentes = Vec::new();
        let extras = self.extras.clone().unwrap_or_default();

        let mut considerar = |activo: bool, nombre: &'static str, artefacto: &Option<Artifact>| {
            if !activo {
                return;
            }
            match artefacto {
                Some(_) => disponibles.push(nombre),
                None => ausentes.push(nombre),
            }
        };

        considerar(optionals.ollama, "ollama", &extras.ollama);
        considerar(optionals.monitoring, "prometheus", &extras.prometheus);
        considerar(optionals.monitoring, "grafana", &extras.grafana);
        considerar(
            optionals.monitoring,
            "windows-exporter",
            &extras.windows_exporter,
        );

        let resueltos = disponibles
            .into_iter()
            .filter_map(|nombre| self.extra(nombre).map(|a| (nombre, a)))
            .collect();

        (resueltos, ausentes)
    }

    /// Artefacto de un extra por su nombre.
    pub fn extra(&self, nombre: &str) -> Option<&Artifact> {
        let extras = self.extras.as_ref()?;
        match nombre {
            "ollama" => extras.ollama.as_ref(),
            "prometheus" => extras.prometheus.as_ref(),
            "grafana" => extras.grafana.as_ref(),
            "windows-exporter" => extras.windows_exporter.as_ref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Profile;

    fn json(schema: u32, url_servidor: &str, sha_servidor: &str) -> String {
        format!(
            r#"{{
              "schema": {schema},
              "channel": "stable",
              "keirost": {{ "version": "1.2.0", "platformRef": "v1.2.0", "releasedAt": "2026-07-27T10:00:00Z" }},
              "artifacts": {{
                "server":   {{ "file": "keirost-server-1.2.0-win-x64.zip", "version": "1.2.0", "url": "{url_servidor}", "sha256": "{sha_servidor}", "size": 180000000 }},
                "web":      {{ "file": "keirost-web-1.2.0.zip", "version": "1.2.0", "url": "https://example.test/web.zip", "sha256": "{sha}", "size": 6000000 }},
                "chromium": {{ "file": "keirost-chromium-131.0.zip", "version": "131.0", "url": "https://example.test/chromium.zip", "sha256": "{sha}", "size": 150000000 }},
                "node":     {{ "file": "node-v20.19.0-win-x64.zip", "version": "20.19.0", "url": "https://example.test/node.zip", "sha256": "{sha}", "size": 30000000 }},
                "postgres": {{ "file": "postgresql-15.14-1-windows-x64-binaries.zip", "version": "15.14-1", "url": "https://example.test/pg.zip", "sha256": "{sha}", "size": 120000000 }}
              }}
            }}"#,
            sha = "b".repeat(64),
        )
    }

    fn manifest_valido() -> String {
        json(1, "https://example.test/server.zip", &"a".repeat(64))
    }

    #[test]
    fn las_versiones_salen_de_las_etiquetas_de_artefactos() {
        // El mismo repositorio publica los instaladores con otro prefijo, y el
        // canal beta usa una etiqueta fija que no nombra versión ninguna:
        // ofrecerlas sería ofrecer algo que no se puede instalar.
        let json = r#"[
            {"tag_name": "keirost-v0.0.12"},
            {"tag_name": "setup-v0.0.12"},
            {"tag_name": "keirost-v0.0.11"},
            {"tag_name": "beta"},
            {"tag_name": "keirost-v0.0.8"}
        ]"#;

        assert_eq!(versiones_de(json), vec!["0.0.12", "0.0.11", "0.0.8"]);
    }

    #[test]
    fn una_respuesta_que_no_se_entiende_no_deja_sin_instalar() {
        // Sin lista se puede seguir escribiendo la versión a mano: quedarse
        // sin instalador por no poder pintar un desplegable sería absurdo.
        assert!(versiones_de("no es json").is_empty());
        assert!(versiones_de("{}").is_empty());
    }

    #[test]
    fn se_puede_pedir_una_version_concreta_y_no_solo_la_ultima() {
        // Instalar «lo último» no vale cuando hay que dejar un equipo igual que
        // otro, ni cuando una versión nueva rompe algo y hay que volver atrás.
        let url = url_para("stable", Some("0.0.8"));

        assert!(url.ends_with("/keirost-v0.0.8/manifest.json"), "{url}");
        assert!(url.starts_with("https://"), "{url}");
    }

    #[test]
    fn sin_version_se_coge_la_ultima_del_canal() {
        assert_eq!(url_para("stable", None), url_for_channel("stable"));
        assert_eq!(url_para("beta", None), url_for_channel("beta"));
    }

    #[test]
    fn una_version_que_no_es_la_pedida_se_rechaza() {
        // Si la etiqueta existe pero trae otra cosa dentro, instalarla en
        // silencio sería peor que fallar: el equipo acabaría con una versión
        // que nadie eligió.
        let manifest = Manifest::parse(&manifest_valido()).unwrap();

        assert!(matches!(
            comprobar_version(manifest, Some("9.9.9")),
            Err(crate::Error::VersionNotFound { .. })
        ));
    }

    #[test]
    fn lee_un_manifest_del_pipeline() {
        let manifest = Manifest::parse(&manifest_valido()).unwrap();

        assert_eq!(manifest.keirost.version, "1.2.0");
        assert_eq!(manifest.artifacts.postgres.version, "15.14-1");
        assert_eq!(manifest.all().len(), 5);
    }

    #[test]
    fn rechaza_un_formato_que_no_entiende() {
        // Un manifest más nuevo puede traer campos obligatorios que este
        // instalador ignoraría en silencio: mejor pedir que se actualice.
        let error =
            Manifest::parse(&json(2, "https://example.test/s.zip", &"a".repeat(64))).unwrap_err();
        assert!(matches!(
            error,
            crate::Error::ManifestVersion { found: 2, .. }
        ));
    }

    #[test]
    fn rechaza_descargas_sin_https() {
        let error = Manifest::parse(&json(1, "http://example.test/server.zip", &"a".repeat(64)))
            .unwrap_err();
        assert!(error.to_string().contains("HTTPS"));
    }

    #[test]
    fn admite_http_solo_contra_el_propio_equipo() {
        // Es lo que permite probar la instalación completa con artefactos
        // servidos en local, sin publicar nada y sin abrir un agujero: en el
        // bucle local no hay red que interceptar.
        assert!(Manifest::parse(&json(
            1,
            "http://127.0.0.1:8000/keirost-server.zip",
            &"a".repeat(64)
        ))
        .is_ok());
        assert!(Manifest::parse(&json(
            1,
            "http://192.168.1.50:8000/keirost-server.zip",
            &"a".repeat(64)
        ))
        .is_err());
    }

    #[test]
    fn rechaza_hashes_que_no_son_sha256() {
        let error = Manifest::parse(&json(1, "https://example.test/s.zip", "1234")).unwrap_err();
        assert!(error.to_string().contains("SHA-256"));
    }

    #[test]
    fn una_release_sin_extras_avisa_en_vez_de_fallar() {
        // Publicar Keirost no debería obligar a republicar Ollama y Grafana:
        // si el extra no está, se instala el resto y se dice cuál falta.
        let manifest = Manifest::parse(&manifest_valido()).unwrap();
        let optionals = crate::settings::Optionals {
            backups: true,
            ollama: true,
            monitoring: true,
        };

        let (disponibles, ausentes) = manifest.extras_for(&optionals);

        assert!(disponibles.is_empty());
        assert_eq!(ausentes.len(), 4, "ollama, prometheus, grafana y exporter");
        assert!(ausentes.contains(&"ollama"));
    }

    #[test]
    fn solo_descarga_los_extras_que_se_han_pedido() {
        let con_extras = manifest_valido().replace(
            "\"artifacts\": {",
            &format!(
                "\"extras\": {{ \"ollama\": {{ \"file\": \"ollama.zip\", \"version\": \"0.6.0\", \"url\": \"https://example.test/ollama.zip\", \"sha256\": \"{sha}\", \"size\": 1 }} }},\n\"artifacts\": {{",
                sha = "c".repeat(64)
            ),
        );
        let manifest = Manifest::parse(&con_extras).unwrap();

        let (disponibles, ausentes) = manifest.extras_for(&crate::settings::Optionals {
            backups: false,
            ollama: true,
            monitoring: false,
        });

        assert_eq!(disponibles.len(), 1);
        assert_eq!(disponibles[0].0, "ollama");
        assert!(ausentes.is_empty(), "no se pidió monitorización");
    }

    #[test]
    fn cada_canal_tiene_su_manifest() {
        assert_eq!(url_for_channel("stable"), STABLE_URL);
        assert_eq!(url_for_channel("BETA"), BETA_URL);
        // Un canal desconocido cae en estable en vez de fallar: es preferible
        // instalar lo probado a dejar el wizard bloqueado.
        assert_eq!(url_for_channel("inventado"), STABLE_URL);
    }

    #[test]
    fn el_perfil_de_escritorio_no_descarga_el_servidor() {
        let manifest = Manifest::parse(&manifest_valido()).unwrap();

        assert!(manifest.required_for(Profile::Desktop).is_empty());
        assert_eq!(manifest.required_for(Profile::Server).len(), 5);
        assert_eq!(manifest.total_size(Profile::Desktop), 0);
        assert!(manifest.total_size(Profile::Full) > 400_000_000);
    }
}
