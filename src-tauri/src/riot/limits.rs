/* limits.rs — controle de ritmo das chamadas.
 *
 * O app conversa com APIs locais e com os endpoints autenticados da Riot.
 * Rajadas (um roster inteiro de MMR em paralelo, ticks de 1 s) criam um
 * padrão de tráfego que nenhum cliente oficial produz. Aqui toda chamada
 * passa por um pacer: intervalo mínimo entre requests + jitter, e um teto
 * por janela de 60 s. Nada de burst, nada de metralhadora.
 *
 * Jitter usa xorshift semeado uma vez com o relógio do sistema — não vale
 * puxar `rand` só para isso.
 */

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

static SEED: AtomicU64 = AtomicU64::new(0);

/// 0..max_ms, não criptográfico — só para não ter cadência de metrônomo.
pub fn jitter(max_ms: u64) -> u64 {
    if max_ms == 0 {
        return 0;
    }
    let mut x = SEED.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9_7f4a_7c15)
            | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    SEED.store(x, Ordering::Relaxed);
    x % max_ms.max(1)
}

/// Serializa as chamadas: cada `acquire` espera o intervalo mínimo desde
/// a anterior, mais um jitter curto. O mutex é segurado durante o sleep de
/// propósito — é ele que garante o espaçamento global.
pub struct Pacer {
    min_gap: Duration,
    last: Mutex<Option<Instant>>,
}

impl Pacer {
    pub const fn new(min_gap_ms: u64) -> Self {
        Self {
            min_gap: Duration::from_millis(min_gap_ms),
            last: Mutex::const_new(None),
        }
    }

    async fn wait_gap(&self) {
        let mut last = self.last.lock().await;
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < self.min_gap {
                let wait = (self.min_gap - elapsed).as_millis() as u64;
                tokio::time::sleep(Duration::from_millis(wait + jitter(90))).await;
            } else {
                tokio::time::sleep(Duration::from_millis(jitter(40))).await;
            }
        }
        *last = Some(Instant::now());
    }
}

/// Teto de requests por janela deslizante fixa (60 s). Estourou, dorme o
/// resto da janela em vez de falhar: o próximo tick pega o que faltou.
pub struct Budget {
    max: u32,
    /* (início da janela, usados). `None` = janela ainda não abriu; a
     * primeira chamada inicializa sem depender de Instant em const. */
    used: Mutex<Option<(Instant, u32)>>,
}

impl Budget {
    pub const fn new(max: u32) -> Self {
        Self {
            max,
            used: Mutex::const_new(None),
        }
    }

    async fn take(&self) {
        let mut st = self.used.lock().await;
        if st.is_none() {
            *st = Some((Instant::now(), 0));
        }
        let (since, count) = st.as_mut().expect("inicializado acima");
        if since.elapsed() >= Duration::from_secs(60) {
            *since = Instant::now();
            *count = 0;
        }
        if *count >= self.max {
            let left = Duration::from_secs(60).saturating_sub(since.elapsed());
            tokio::time::sleep(left + Duration::from_millis(jitter(250))).await;
            *since = Instant::now();
            *count = 0;
        }
        *count += 1;
    }
}

pub struct Limiter {
    pacer: Pacer,
    budget: Budget,
}

impl Limiter {
    pub const fn new(min_gap_ms: u64, per_minute: u32) -> Self {
        Self {
            pacer: Pacer::new(min_gap_ms),
            budget: Budget::new(per_minute),
        }
    }

    pub async fn acquire(&self) {
        self.pacer.wait_gap().await;
        self.budget.take().await;
    }
}

/// API local do Riot Client: barata e mesma máquina, mas ainda é tráfego.
pub static LOCAL: Limiter = Limiter::new(45, 240);

/// glz/pd: endpoints autenticados da Riot. Cadência folgada e teto baixo.
pub static REMOTE: Limiter = Limiter::new(160, 60);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_stays_in_range() {
        for _ in 0..200 {
            assert!(jitter(50) < 50);
        }
        assert_eq!(jitter(0), 0);
    }
}
