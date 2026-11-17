//! Compatibilidad entre la TUI (Python) y el núcleo de la GUI (Rust): mismos
//! archivos, mismo formato, sin perder campos y sin corromperse con escrituras
//! simultáneas. Lanza el código real de Python (`tests/compat_helper.py`) sobre
//! un HOME/XDG aislado. Se omite si no hay Python.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use serde_json::{json, Value};
use zeroagents_core::claude_code::ClaudeCode;
use zeroagents_core::{jsonfile, work};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn python() -> Option<PathBuf> {
    let venv = repo().join(".venv/bin/python");
    let candidates = [venv, PathBuf::from("python3")];
    candidates.into_iter().find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()))
}

struct Env {
    root: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("za-compat-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("home")).unwrap();
        Env { root }
    }
    fn state(&self) -> PathBuf {
        self.root.join("state")
    }
    fn claude(&self) -> PathBuf {
        self.root.join("claude")
    }
    fn registry(&self) -> PathBuf {
        self.state().join("work/projects.json")
    }
    fn py_cmd(&self, py: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new(py);
        cmd.arg(repo().join("tests/compat_helper.py")).args(args);
        cmd.env("HOME", self.root.join("home"))
            .env("ZEROAGENTS_STATE_DIR", self.state())
            .env("CLAUDE_CONFIG_DIR", self.claude());
        cmd
    }
    fn py(&self, py: &Path, args: &[&str]) -> Output {
        let out = self.py_cmd(py, args).output().unwrap();
        assert!(out.status.success(), "python {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        out
    }
    fn registry_json(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.registry()).unwrap()).expect("projects.json válido")
    }
    fn plan_for(&self, dir: &Path) {
        fs::create_dir_all(dir.join(".zeroagents")).unwrap();
        fs::write(dir.join(work::PLAN_FILE), "{}").unwrap();
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn dirs_of(v: &Value) -> Vec<String> {
    v["projects"].as_array().unwrap().iter().filter_map(|p| p.get("dir")?.as_str().map(String::from)).collect()
}

#[test]
fn python_escribe_y_rust_lee_la_sesion() {
    let Some(py) = python() else { return };
    let env = Env::new("sesion");
    env.py(&py, &["ingest"]);
    let py_view: Value = serde_json::from_slice(&env.py(&py, &["collect"]).stdout).unwrap();

    let mut adapter = ClaudeCode::with_dirs(env.state().join("claude-code"), env.claude());
    let (agents, limits) = adapter.collect(true);
    assert_eq!(agents.len(), 1);
    let a = &agents[0];
    let py_a = &py_view[0];
    assert_eq!(a.id, py_a["id"].as_str().unwrap());
    assert_eq!(a.status.label(), py_a["status"].as_str().unwrap());
    assert_eq!(a.model.as_deref(), py_a["model"].as_str());
    assert_eq!(a.effort.as_deref(), py_a["effort"].as_str());
    assert_eq!(a.usage.cost_usd, py_a["cost"].as_f64());
    assert_eq!(a.usage.context_pct, py_a["context_pct"].as_f64());
    assert_eq!(a.cwd, py_a["cwd"].as_str().unwrap());
    assert_eq!(a.last_message.as_deref(), py_a["last_message"].as_str());
    assert_eq!(limits.unwrap().five_hour_pct, Some(30.0));
}

#[test]
fn registro_cruzado_conserva_campos_desconocidos() {
    let Some(py) = python() else { return };
    let env = Env::new("cruzado");
    let (a, b, c) = (env.root.join("a"), env.root.join("b"), env.root.join("c"));
    for d in [&a, &b, &c] {
        env.plan_for(d);
    }
    // Un archivo «del futuro»: versión 7, campo de nivel superior y campo propio de un proyecto.
    fs::create_dir_all(env.state().join("work")).unwrap();
    let future = json!({"version": 7, "futuro": {"x": [1, 2]}, "projects": [
        {"dir": a.to_string_lossy(), "name": "A", "added": 1.0, "color": "azul"},
        "no soy un objeto"]});
    fs::write(env.registry(), future.to_string()).unwrap();

    // Python registra otro; Rust vuelve a registrar `a` y otro más.
    env.py(&py, &["register", b.to_str().unwrap(), "B"]);
    work::register(&env.state(), &a, "A renombrado").unwrap();
    work::register(&env.state(), &c, "C").unwrap();
    env.py(&py, &["register", b.to_str().unwrap(), "B2"]);

    let v = env.registry_json();
    assert_eq!(v["version"], 7, "no se rebaja la versión");
    assert_eq!(v["futuro"], json!({"x": [1, 2]}), "campo de nivel superior conservado");
    let items = v["projects"].as_array().unwrap();
    let item_a = items.iter().find(|p| p["dir"] == json!(a.to_string_lossy())).unwrap();
    assert_eq!(item_a["color"], "azul", "campo propio del proyecto conservado");
    assert_eq!(item_a["name"], "A renombrado");
    assert!(items.contains(&json!("no soy un objeto")), "elementos raros se respetan");
    assert_eq!(dirs_of(&v).len(), 3, "a, b y c sin duplicados (el elemento raro no cuenta)");

    // Ambas implementaciones ven lo mismo y en el mismo orden (el más reciente primero).
    let from_py: Vec<String> = serde_json::from_slice(&env.py(&py, &["projects"]).stdout).unwrap();
    let from_rust: Vec<String> = work::projects(&env.state()).iter().map(|p| p.to_string_lossy().into()).collect();
    assert_eq!(from_py, from_rust);
    assert_eq!(from_rust[0], b.to_string_lossy());
}

#[test]
fn migracion_v0_a_v1_guarda_bak_y_no_pierde_nada() {
    let Some(py) = python() else { return };
    for who in ["rust", "python"] {
        let env = Env::new(&format!("mig-{who}"));
        fs::create_dir_all(env.state().join("work")).unwrap();
        let old = json!({"projects": [{"dir": "/viejo", "name": "Viejo", "added": 5.0, "nota": "x"}], "otro": true});
        fs::write(env.registry(), old.to_string()).unwrap();
        if who == "rust" {
            work::register(&env.state(), Path::new("/nuevo"), "Nuevo").unwrap();
        } else {
            env.py(&py, &["register", "/nuevo", "Nuevo"]);
        }
        let v = env.registry_json();
        assert_eq!(v["version"], 1, "{who}");
        assert_eq!(v["otro"], true, "{who}");
        let bak: Value = serde_json::from_str(&fs::read_to_string(env.state().join("work/projects.json.bak")).unwrap()).unwrap();
        assert_eq!(bak, old, "{who}: la copia es el original");
        let viejo = v["projects"].as_array().unwrap().iter().find(|p| p["dir"] == "/viejo").unwrap();
        assert_eq!(viejo["nota"], "x", "{who}");
        // Una segunda escritura ya no migra: el .bak no se pisa.
        if who == "rust" {
            work::register(&env.state(), Path::new("/otro"), "Otro").unwrap();
        } else {
            env.py(&py, &["register", "/otro", "Otro"]);
        }
        let bak2: Value = serde_json::from_str(&fs::read_to_string(env.state().join("work/projects.json.bak")).unwrap()).unwrap();
        assert_eq!(bak2, old, "{who}: el .bak sigue siendo el original");
    }
}

#[test]
fn archivo_corrupto_se_aparta_en_ambas() {
    let Some(py) = python() else { return };
    for who in ["rust", "python"] {
        let env = Env::new(&format!("corrupto-{who}"));
        fs::create_dir_all(env.state().join("work")).unwrap();
        fs::write(env.registry(), "{esto no es json").unwrap();
        if who == "rust" {
            work::register(&env.state(), Path::new("/p"), "P").unwrap();
        } else {
            env.py(&py, &["register", "/p", "P"]);
        }
        assert_eq!(dirs_of(&env.registry_json()), ["/p"]);
        let apartado = fs::read_dir(env.state().join("work"))
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().contains("projects.json.corrupto-"))
            .unwrap_or_else(|| panic!("{who}: falta la copia del archivo corrupto"));
        assert_eq!(fs::read_to_string(apartado.path()).unwrap(), "{esto no es json");
    }
}

#[test]
fn escrituras_simultaneas_de_ambos_no_corrompen_ni_pierden() {
    let Some(py) = python() else { return };
    let env = Env::new("concurrente");
    const PER: usize = 25;
    let stop = Arc::new(AtomicBool::new(false));

    // Un lector que lee sin bloqueo todo el rato: nunca debe ver un JSON a medias.
    let reg_path = env.registry();
    let reader = {
        let stop = stop.clone();
        thread::spawn(move || {
            let mut reads = 0u32;
            while !stop.load(Ordering::Relaxed) {
                if let Ok(text) = fs::read_to_string(&reg_path) {
                    serde_json::from_str::<Value>(&text).expect("lectura de JSON a medias");
                    reads += 1;
                }
            }
            reads
        })
    };

    let py_procs: Vec<_> = (0..3)
        .map(|i| {
            let base = env.root.join(format!("py{i}"));
            env.py_cmd(&py, &["register", base.to_str().unwrap(), &format!("py{i}"), &PER.to_string()])
                .spawn()
                .unwrap()
        })
        .collect();
    let rust_threads: Vec<_> = (0..3)
        .map(|i| {
            let (state, base) = (env.state(), env.root.join(format!("rs{i}")));
            thread::spawn(move || {
                for n in 0..PER {
                    work::register(&state, &PathBuf::from(format!("{}-{n}", base.display())), &format!("rs{i}-{n}")).unwrap();
                }
            })
        })
        .collect();
    for t in rust_threads {
        t.join().unwrap();
    }
    for mut p in py_procs {
        assert!(p.wait().unwrap().success());
    }
    stop.store(true, Ordering::Relaxed);
    assert!(reader.join().unwrap() > 0);

    let v = env.registry_json();
    let dirs = dirs_of(&v);
    assert_eq!(dirs.len(), 6 * PER, "no se pierde ninguna alta");
    let unique: std::collections::HashSet<_> = dirs.iter().collect();
    assert_eq!(unique.len(), dirs.len());
    // Ningún temporal huérfano.
    let leftovers: Vec<_> = fs::read_dir(env.state().join("work"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temporales sin limpiar: {leftovers:?}");
}

#[test]
fn libro_de_subagentes_concurrente_con_python_y_bloqueo_compartido() {
    let Some(py) = python() else { return };
    let env = Env::new("ledger");
    let procs: Vec<_> = (0..4)
        .map(|i| env.py_cmd(&py, &["ledger", "s1", &format!("agente{i}"), "20"]).spawn().unwrap())
        .collect();
    for mut p in procs {
        assert!(p.wait().unwrap().success());
    }
    let v: Value = serde_json::from_str(&fs::read_to_string(env.state().join("claude-code/s1.agents.json")).unwrap()).unwrap();
    assert_eq!(v["bg"].as_array().unwrap().len(), 80);

    // El bloqueo de Rust excluye al de Python: con el lock tomado, Python espera.
    let target = env.state().join("claude-code/s1.agents.json");
    let guard = jsonfile::lock(&target).unwrap();
    let mut child = env.py_cmd(&py, &["ledger", "s1", "tarde", "1"]).spawn().unwrap();
    thread::sleep(std::time::Duration::from_millis(700));
    assert!(child.try_wait().unwrap().is_none(), "Python no respeta el flock de Rust");
    drop(guard);
    assert!(child.wait().unwrap().success());
    let v: Value = serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(v["bg"].as_array().unwrap().len(), 81);
}
