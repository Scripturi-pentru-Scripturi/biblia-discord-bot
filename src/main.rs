use serde_json::json;
use serde_json::{Map, Value};
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::error::Error;
use std::{env, fs};
use strsim::jaro_winkler;

struct Handler;

fn find_book_name(bible: &Map<String, Value>, name: &str) -> String {
    let mut best_score = 0.0;
    let mut best_book = "";
    for (book_name, book_data) in bible.iter() {
        let score = jaro_winkler(name, book_name);
        if score > best_score {
            best_score = score;
            best_book = book_name;
        }
        if let Some(alternatives) = book_data.get("alternative").and_then(Value::as_array) {
            for alternative in alternatives.iter().filter_map(Value::as_str) {
                let score = jaro_winkler(name, alternative);
                if score > best_score {
                    best_score = score;
                    best_book = book_name;
                }
            }
        }
    }

    if best_score == 0.0 {
        eprintln!("Nu am gasit nici o carte cu numele '{}' in biblie.", name);
        return bible.keys().next().unwrap().to_string();
    }

    best_book.to_string()
}

fn get_verses(bible: &Map<String, Value>, book: &str, chapter: usize, start_verse: usize, end_verse: usize) -> Option<Vec<String>> {
    bible
        .get(book)
        .and_then(|book_data| book_data.get("capitole").and_then(Value::as_object))
        .and_then(|chapters| chapters.get(&chapter.to_string()).and_then(Value::as_object))
        .and_then(|verses_data| verses_data.get("versete").and_then(Value::as_array))
        .map(|verses| {
            verses
                .iter()
                .enumerate()
                .skip(start_verse - 1)
                .take(end_verse - start_verse + 1)
                .filter_map(|(_i, verse)| {
                    let verset_num = verse.get("verset").and_then(Value::as_u64)?;
                    let text = verse.get("text").and_then(Value::as_str)?;
                    Some(format!("> **{}:{}** {}", chapter, verset_num, text))
                })
                .collect()
        })
}

fn parse_reference(reference: &str, llm: bool) -> (&str, usize, (usize, usize)) {
    let parts: Vec<&str> = reference.split(':').collect();
    let book_name = parts.get(0).expect("Cartea nu e specificata");
    let chapter_arg = parts.get(1).expect("Capitolul nu e specificat");
    let chapter: usize = chapter_arg.parse().expect("Capitolul nu e in format valid");
    let mut start_verse = 1;
    let mut end_verse = usize::MAX;
    if parts.len() == 3 {
        if let Some(dash) = parts[2].find('-') {
            let (start, end) = parts[2].split_at(dash);
            start_verse = start.parse().expect("Versetul de inceput nu e in format valid");
            end_verse = end[1..].parse().expect("Versetul de sfarsit nu e in format valid");
        } else {
            start_verse = parts[2].parse().expect("Versetul nu e in format valid");
            end_verse = start_verse;
        }
    } else if parts.len() > 3 {
        panic!("Referinta nu e in format valid, foloseste ':' pentru a separa capitolul de verset si '-' pentru a separa versetele");
    }
    (*book_name, chapter, (start_verse, end_verse))
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content.starts_with("!biblia-llm") {
            let _ = msg.content.strip_prefix("!biblia-llm");
            let api_key = dotenv::var("OPENAI_API_KEY").expect("Cheia de API nu e setata in varibila OPENAI_API_KEY.");
            let model = "gpt-3.5-turbo";
            let input = msg.content.strip_prefix("!test-llm").expect("Mesajul nu e in format valid");
            let response = llm(&api_key, input, model).expect("Nu am putut obtine raspunsul de la LLM");
            let bible_json = fs::read_to_string("biblia.json").unwrap();
            let bible: Map<String, Value> = serde_json::from_str(&bible_json).expect("Nu am putut citi biblia");
            let (book_name, chapter_number, verse_range) = parse_reference(&response, true);
            let found_book = find_book_name(&bible, book_name);
            let verses = get_verses(&bible, &found_book, chapter_number, verse_range.0, verse_range.1);
            if let Some(verses) = verses {
                let response = format!("## {}\n ### Capitolul {}\n {}", found_book, chapter_number, verses.join("\n"));
                if let Err(why) = msg.channel_id.say(&ctx.http, response).await {
                    println!("Error sending message: {why:?}");
                }
            } else {
                if let Err(why) = msg.channel_id.say(&ctx.http, "Nu am putut gasi versetul").await {
                    println!("Error sending message: {why:?}");
                }
            }
            return;
        }
        if msg.content.starts_with("!biblia") {
            let _ = msg.content.strip_prefix("!biblia");
            let bible_json = fs::read_to_string("biblia.json").unwrap();
            let bible: Map<String, Value> = serde_json::from_str(&bible_json).expect("Nu am putut citi biblia");
            let (book_name, chapter_number, verse_range) = parse_reference(&msg.content, false);
            let found_book = find_book_name(&bible, book_name);
            println!("Book: {}", found_book);
            let verses = get_verses(&bible, &found_book, chapter_number, verse_range.0, verse_range.1);
            println!("Verses: {:?}", verses);

            if let Some(verses) = verses {
                let response = format!("## {}\n ### Capitolul {}\n {}", found_book, chapter_number, verses.join("\n"));
                if let Err(why) = msg.channel_id.say(&ctx.http, response).await {
                    println!("Error sending message: {why:?}");
                }
            } else {
                if let Err(why) = msg.channel_id.say(&ctx.http, "Nu am putut gasi versetul").await {
                    println!("Error sending message: {why:?}");
                }
            }
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

fn llm(api_key: &str, input: &str, model: &str) -> Result<String, Box<dyn Error>> {
    let client = reqwest::blocking::Client::new();
    let system_prompt = "Esti un preot ortodox si imi raspunzi cu o singura referinta din biblia ortodoxa (atentie la psalmi!) Fc,Ies,Lv,Num,Dt,Ios,Jd,Rut,1Rg,2Rg,3Rg,4Rg,1Par,2Par,1Ezr,Ne,Est,Iov,Ps,Pr,Ecc,Cant,Is,Ir,Plg,Iz,Dn,Os,Am,Mi,Ioil,Avd,Ion,Naum,Avc,Sof,Ag,Za,Mal,Tob,Idt,Bar,Epist,Tin,3Ezr,Sol,Sir,Sus,Bel,1Mac,2Mac,3Mac,Man,Mt,Mc,Lc,In,FA,Rm,1Co,2Co,Ga,Ef,Flp,Col,1Tes,2Tes,1Tim,2Tim,Tit,Flm,Evr,Iac,1Ptr,2Ptr,1In,2In,3In,Iuda,Ap pe subiectul indicat. nu spui altceva inafara de referinta. formatul referintei este: Mt:10:20 sau Lc:20:2-3 sau Ap:1:2-4";
    let request_body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": input}
        ]
    });

    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&request_body)
        .send()?;

    let response_text = response.json::<serde_json::Value>()?;

    match response_text.get("error").and_then(|e| e.as_str()) {
        Some(error) => Err(error.into()),
        None => {
            let answer = response_text["choices"][0]["message"]["content"].as_str().ok_or("No content found")?;
            Ok(answer.to_string())
        }
    }
}

#[tokio::main]
async fn main() {
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::DIRECT_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&token, intents).event_handler(Handler).await.expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }
}
