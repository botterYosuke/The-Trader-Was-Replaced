use bevy::prelude::*;
use rand::Rng;

#[derive(Resource)]
pub struct TradingData {
    pub price: f32,
    pub history: Vec<f32>,
    pub timer: Timer,
}

impl Default for TradingData {
    fn default() -> Self {
        Self {
            price: 100.0,
            history: vec![100.0],
            timer: Timer::from_seconds(0.5, TimerMode::Repeating),
        }
    }
}

pub fn price_simulation_system(
    time: Res<Time>,
    mut data: ResMut<TradingData>,
) {
    data.timer.tick(time.delta());
    if data.timer.just_finished() {
        let mut rng = rand::thread_rng();
        let change = rng.gen_range(-0.5..0.6);
        data.price += change;
        let price = data.price;
        data.history.push(price);
        if data.history.len() > 50 {
            data.history.remove(0);
        }
    }
}
