use serde_json::{Map, Value};
use serenity::{
    async_trait,
    model::{channel::Message, gateway::Ready},
    prelude::*,
};
use std::{env, fs};
use strsim::jaro_winkler;

struct Handler {
    bible: Map<String, Value>,
}

impl Handler {
    fn new(bible_file: &str) -> Self {
        let data =
            fs::read_to_string(bible_file).expect("Eroare in citirea fisierului biblia.json.");
        let json: Value = serde_json::from_str(&data).expect("Eroare in parsare JSON");
        let bible = json.as_object().expect("JSON nu este obiect").clone();
        Handler { bible }
    }
    fn get_verses(
        &self,
        book: &str,
        chapter: usize,
        start_verse: usize,
        end_verse: usize,
    ) -> Option<String> {
        self.bible
            .get(book)
            .and_then(|b| b.get("capitole").and_then(Value::as_object))
            .and_then(|c| c.get(&chapter.to_string()).and_then(Value::as_object))
            .and_then(|v| v.get("versete").and_then(Value::as_array))
            .and_then(|verses| {
                let verse_texts: Vec<String> = verses
                    .iter()
                    .enumerate()
                    .skip(start_verse - 1)
                    .take(end_verse - start_verse + 1)
                    .filter_map(|(i, verse)| {
                        let verse_num = verse.get("verset").and_then(Value::as_u64)?;
                        let text = verse.get("text").and_then(Value::as_str)?;
                        Some(format!("* {} {}", verse_num, text))
                    })
                    .collect();
                if verse_texts.is_empty() {
                    None
                } else {
                    Some(verse_texts.join("\n"))
                }
            })
            .map(|verses_text| {
                format!(
                    "> # {}\n> ## Capitolul: {}\n {}",
                    book, chapter, verses_text
                )
            })
    }

    fn find_book_name(&self, input: &str) -> Option<String> {
        let mut best_match: Option<(String, f64)> = None;
        for (book, data) in &self.bible {
            if book == input {
                return Some(book.to_string());
            }
            if let Some(alternatives) = data.get("alternative").and_then(|v| v.as_array()) {
                for alt in alternatives.iter().filter_map(|v| v.as_str()) {
                    if alt == input {
                        return Some(book.to_string());
                    }
                }
            }
        }
        for (book, _) in &self.bible {
            let score = jaro_winkler(input, book);
            if best_match.is_none() || score > best_match.as_ref().unwrap().1 {
                best_match = Some((book.to_string(), score));
            }

            if let Some(alternatives) = self
                .bible
                .get(book)
                .and_then(|b| b.get("alternative").and_then(|a| a.as_array()))
            {
                for alternative in alternatives.iter().filter_map(|a| a.as_str()) {
                    let score = jaro_winkler(input, alternative);
                    if best_match.is_none() || score > best_match.as_ref().unwrap().1 {
                        best_match = Some((book.to_string(), score));
                    }
                }
            }
        }

        best_match.map(|(book, _)| book)
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if let Some(content) = msg.content.strip_prefix("!biblia") {
            let parts: Vec<&str> = content.split(':').map(str::trim).collect();
            if let (Some(book), Some(chapter), Some(verse_info)) =
                (parts.get(0), parts.get(1), parts.get(2))
            {
                let chapter: usize = chapter.parse().expect("Invalid chapter number.");
                let (start_verse, end_verse) = parse_verse_info(verse_info);

                if let Some(book_key) = self.find_book_name(book) {
                    if let Some(verses) =
                        self.get_verses(&book_key, chapter, start_verse, end_verse)
                    {
                        if let Err(why) = msg.channel_id.say(&ctx.http, &verses).await {
                            eprintln!("Error sending message: {:?}", why);
                        }
                    } else {
                        if let Err(why) = msg
                            .channel_id
                            .say(&ctx.http, "Couldn't find the verse(s).")
                            .await
                        {
                            eprintln!("Error sending message: {:?}", why);
                        }
                    }
                } else {
                    if let Err(why) = msg
                        .channel_id
                        .say(&ctx.http, "Couldn't find the book.")
                        .await
                    {
                        eprintln!("Error sending message: {:?}", why);
                    }
                }
            }
        }
        fn parse_verse_info(verse_info: &str) -> (usize, usize) {
            let verses: Vec<&str> = verse_info.split('-').collect();
            let start_verse = verses[0].parse().unwrap_or(0);
            let end_verse = verses
                .get(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(start_verse);
            (start_verse, end_verse)
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} este connectat!", ready.user.name);
    }
}

#[tokio::main]
async fn main() {
    let token = env::var("DISCORD_TOKEN")
        .expect("Nu am gasit tokenul botului de in enviroment var DISCORD_TOKEN");
    let bible_file = "biblia.json";

    let handler = Handler::new(bible_file);

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .expect("Eroare la crearea clientului");

    if let Err(why) = client.start().await {
        eprintln!("Eroare de la client: {:?}", why);
    }
}
