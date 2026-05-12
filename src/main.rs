use iced::widget::{button, column, container, row, text, space, canvas};
use iced::{Alignment, Element, Length, Theme, Color, Point, Rectangle, Renderer, Center, Subscription, Task};
use iced::mouse;
use std::time::Duration;

pub fn main() -> iced::Result {
    iced::application(|| (State::default(), Task::none()), State::update, State::view)
        .title("Trader Dashboard")
        .subscription(State::subscription)
        .theme(State::theme)
        .run()
}

#[derive(Debug, Clone, Copy)]
enum Message {
    BuyPressed,
    SellPressed,
    Tick,
}

struct State {
    price: f32,
    history: Vec<f32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            price: 100.0,
            history: vec![100.0],
        }
    }
}

impl State {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::BuyPressed => {
                self.price += 1.5;
            }
            Message::SellPressed => {
                self.price -= 1.5;
            }
            Message::Tick => {
                // Simulate some price movement
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let change = rng.gen_range(-0.5..0.6);
                self.price += change;
                self.history.push(self.price);
                if self.history.len() > 100 {
                    self.history.remove(0);
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let title_text = text("TRADER DASHBOARD")
            .size(40)
            .color(Color::from_rgb(0.0, 0.8, 1.0));

        let price_display = text(format!("${:.2}", self.price))
            .size(80)
            .color(if self.price >= 100.0 { Color::from_rgb(0.0, 1.0, 0.0) } else { Color::from_rgb(1.0, 0.0, 0.0) });

        let controls = row![
            button("BUY")
                .padding(15)
                .on_press(Message::BuyPressed),
            button("SELL")
                .padding(15)
                .on_press(Message::SellPressed),
        ]
        .spacing(20);

        let chart = container(
            canvas(PriceChart { history: self.history.clone() })
                .width(Length::Fill)
                .height(Length::Fixed(300.0))
        )
        .padding(20)
        .style(container::rounded_box);

        let content = column![
            title_text,
            space::vertical().height(20),
            price_display,
            space::vertical().height(40),
            chart,
            space::vertical().height(40),
            controls,
        ]
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .padding(40);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Center)
            .align_y(Center)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick)
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}

struct PriceChart {
    history: Vec<f32>,
}

impl<Message> canvas::Program<Message> for PriceChart {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        if self.history.len() < 2 {
            return vec![frame.into_geometry()];
        }

        let max_price = self.history.iter().cloned().fold(f32::NEG_INFINITY, f32::max).max(105.0);
        let min_price = self.history.iter().cloned().fold(f32::INFINITY, f32::min).min(95.0);
        let range = (max_price - min_price).max(1.0);

        let x_step = bounds.width / (self.history.len() - 1) as f32;
        
        let points: Vec<Point> = self.history.iter().enumerate().map(|(i, &p)| {
            let x = i as f32 * x_step;
            let y = bounds.height - ((p - min_price) / range * bounds.height);
            Point::new(x, y)
        }).collect();

        let path = canvas::Path::new(|builder| {
            builder.move_to(points[0]);
            for &p in &points[1..] {
                builder.line_to(p);
            }
        });

        frame.stroke(&path, canvas::Stroke::default()
            .with_color(Color::from_rgb(0.0, 0.8, 1.0))
            .with_width(2.0));

        vec![frame.into_geometry()]
    }
}
