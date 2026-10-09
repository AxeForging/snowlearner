//! Every user-facing string of the app, in each native language. One plain
//! table: a key names the text and the macro makes both languages mandatory,
//! so a key without its English (or Portuguese) does not compile.
//! `{0}`, `{1}`… are filled by [`T::fill`].

use super::native::Native;

macro_rules! texts {
    ($($key:ident => $pt:expr, $en:expr;)*) => {
        /// A user-facing text.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum T { $($key,)* }

        impl T {
            pub const ALL: &'static [T] = &[$(T::$key,)*];

            pub fn get(self, native: Native) -> &'static str {
                match (self, native) {
                    $((T::$key, Native::PtBr) => $pt, (T::$key, Native::En) => $en,)*
                }
            }
        }
    };
}

macro_rules! lines {
    ($($key:ident => [$($pt:expr),* $(,)?], [$($en:expr),* $(,)?];)*) => {
        /// A set of lines an actor picks from.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Lines { $($key,)* }

        impl Lines {
            pub const ALL: &'static [Lines] = &[$(Lines::$key,)*];

            pub fn get(self, native: Native) -> &'static [&'static str] {
                match (self, native) {
                    $((Lines::$key, Native::PtBr) => &[$($pt),*], (Lines::$key, Native::En) => &[$($en),*],)*
                }
            }
        }
    };
}

texts! {
    // ---------------------------------------------------------------- panel
    PanelTitle => "SNOWLEARNER · PAINEL", "SNOWLEARNER · PANEL";
    PanelWindow => "Snowlearner · Painel", "Snowlearner · Panel";
    TabGame => "JOGO", "GAME";
    TabAudio => "ÁUDIO", "AUDIO";
    TabProgress => "PROGRESSO", "PROGRESS";
    ItemNative => "Minha língua", "My language";
    ItemLanguage => "Idioma", "Learning";
    ItemCommitment => "Compromisso", "Commitment";
    ItemTopic => "Tema", "Topic";
    ItemLevel => "Nível", "Level";
    ItemPractice => "Prática", "Practice";
    ItemAnswer => "Resposta", "Answer";
    ItemGoal => "Meta diária", "Daily goal";
    ItemMode => "Tela", "Screen";
    ItemPracticeNow => "> Praticar agora", "> Practice now";
    ItemSummary => "> Resumo do dia", "> Today's recap";
    ItemPause => "Mago", "Mage";
    ItemQuit => "> Sair", "> Quit";
    ItemMic => "Microfone", "Microphone";
    ItemTestMic => "> Testar microfone", "> Test microphone";
    ItemEngine => "Motor de voz", "Voice engine";
    ItemEndpoint => "Endereço", "Address";
    ItemVoiceNative => "Minha voz", "My voice";
    ItemVoiceLearning => "Voz do idioma", "Learning voice";
    ItemTestVoices => "> Testar vozes", "> Test voices";
    ItemSpeaker => "Alto-falante", "Speaker";
    AllTopicsShort => "todos", "all";
    TopicsCount => "{0} temas", "{0} topics";
    PickerFooter => "↑↓ Enter: escolher  Esc: voltar", "↑↓ Enter: pick  Esc: back";
    ChecklistFooter => "↑↓ Enter: marcar  Esc: pronto", "↑↓ Enter: tick  Esc: done";
    ChecklistNoneIsAll => "Nada marcado = todos os temas", "Nothing ticked = all topics";
    LevelBeginner => "pré-A1 · iniciante", "pre-A1 · beginner";
    LevelUpTo => "até {0}", "up to {0}";
    PracticeAuto => "automático", "automatic";
    PracticeRepeat => "repetir", "repeat";
    PracticeRecall => "de memória", "from memory";
    GoalPhrases => "{0} frases", "{0} phrases";
    ModeAuto => "auto*", "auto*";
    ModeWindow => "janela*", "window*";
    ModeOverlay => "sobre a tela*", "over the screen*";
    Paused => "pausado", "paused";
    Active => "ativo", "active";
    SystemDefault => "padrão do sistema", "system default";
    EngineSystem => "sistema", "system";
    EngineCommand => "comando", "command";
    VoiceAuto => "automática", "automatic";
    VoiceAutoUses => "Automática, usa {0}", "Automatic, uses {0}";
    VoiceFemale => "voz feminina", "female voice";
    VoiceMale => "voz masculina", "male voice";
    VoiceOld => ", versão antiga", ", old version";
    AddressSaved => "Endereço salvo. Enter: testar as vozes.", "Address saved. Enter: test the voices.";
    AddressInvalid => "Endereço inválido: use http://IP:porta/v1", "Invalid address: use http://IP:port/v1";
    AddressTyping => "Digite o endereço · Enter salva · Esc cancela", "Type the address · Enter saves · Esc cancels";
    SaySomething => "fale algo…", "say something…";
    PanelFooter => "*ao reiniciar  ↑↓ ←→ Enter  Tab: aba  Esc", "*on restart  ↑↓ ←→ Enter  Tab: tab  Esc";
    ProgressFooter => "Tab: aba  Esc: fechar", "Tab: tab  Esc: close";
    Loading => "Carregando...", "Loading...";
    ProgressHead => "Sabe {0} de {1} ({2}%) · aprendendo {3}", "Know {0} of {1} ({2}%) · learning {3}";
    KnowsAll => "Você já sabe tudo desta seleção!", "You already know everything here!";
    LearningNow => "Aprendendo agora:", "Learning now:";
    NextUp => "Depois: {0}", "Next: {0}";
    ProgressReport => "Progresso em {0}: {1} de {2} sabidas ({3}%), {4} aprendendo",
        "Progress in {0}: {1} of {2} known ({3}%), {4} learning";
    StageNow => "Etapa atual: {0}", "Current stage: {0}";
    ByTopic => "Por tema:", "By topic:";
    Today => "Hoje: {0}/{1}  ·  combo x{2}", "Today: {0}/{1}  ·  combo x{2}";
    TestListening => "Ouvindo... fale uma frase em voz alta.", "Listening... say a phrase out loud.";
    TestPlaying => "Tocando as duas vozes...", "Playing both voices...";
    TestThinking => "Analisando...", "Checking...";
    TestHeard => "✓ Ouvi: \"{0}\"", "✓ I heard: \"{0}\"";
    TestNothing => "Não ouvi nada. Confira o microfone escolhido e o volume.",
        "I heard nothing. Check the chosen microphone and the volume.";
    TestError => "Erro: {0}", "Error: {0}";
    TestVoicesDone => "✓ Vozes tocadas. Troque em Minha voz / Voz do idioma.",
        "✓ Voices played. Change them in My voice / Learning voice.";
    // ---------------------------------------------------------------- commitment
    CommitChill => "Tranquilo", "Easy";
    CommitSteady => "Constante", "Steady";
    CommitCommitted => "Comprometido", "Committed";
    CommitRelentless => "Implacável", "Relentless";
    // ---------------------------------------------------------------- answer tiers
    AnswerAll => "todas", "all";
    AnswerShort => "curta", "short";
    AnswerComplete => "completa", "complete";
    AnswerPolished => "polida", "polished";
    // ---------------------------------------------------------------- path stages
    StageWord => "palavra", "word";
    StageChunk => "expressão", "chunk";
    StagePhrase => "frase", "phrase";
    StageWords => "palavras", "words";
    StageChunks => "expressões", "chunks";
    StagePhrases => "frases", "phrases";
    WelcomeWords => "Primeiras palavras!", "First words!";
    WelcomeChunks => "Nova etapa: expressões! Agora você junta palavras.", "New stage: chunks! Now you put words together.";
    WelcomePhrases => "Nova etapa: frases! Você já monta frases inteiras.", "New stage: phrases! You build whole sentences now.";
    // ---------------------------------------------------------------- deck cues
    // Ends a situation in the repeat cue: "You walk in. Say: Hola."
    CueSay => "Diga:", "Say:";
    CueRepeat => "Repita:", "Repeat:";
    // Recall cue: the meaning, asked in the target language.
    RecallAsk => "Diga em {0}: \"{1}\"", "Say in {0}: \"{1}\"";
    // ---------------------------------------------------------------- lesson
    TipHotkey => "Aperte {0} e fale uma frase pra me esquentar!", "Press {0} and say a phrase to warm me up!";
    TipFreeze => "Se a neve subir demais eu congelo! {0} e fale!", "If the snow gets too deep I freeze! {0} and speak!";
    TipRecap => "No fim do dia eu leio tudo o que você praticou ({0}).", "At the end of the day I read back all you practiced ({0}).";
    TipSun => "Acerte {0} seguidas e o sol aparece!", "Get {0} in a row and the sun comes out!";
    TipPrefix => "Dica: {0}", "Tip: {0}";
    TagRepeat => "repita", "repeat";
    TagRecall => "de memória", "from memory";
    AllTopics => "todos os temas", "all topics";
    PausedToast => "Em pausa: botão direito no orbe (ou P) para voltar", "Paused: right-click the orb (or P) to come back";
    Analyzing => "Ok, analisando...", "Ok, checking...";
    NoPhrases => "Nenhuma frase com esse tema/nível. Mude no menu.", "No phrase with this topic/level. Change it in the panel.";
    FooterDone => "Terminou? {0} · Esc: cancelar", "Done? {0} · Esc: cancel";
    FooterConfirm => "Fale em voz alta e aperte {0} para confirmar", "Say it out loud and press {0} to confirm";
    VoiceUnavailable => "Voz indisponível: {0}", "Voice unavailable: {0}";
    MicUnavailable => "Microfone/reconhecimento indisponível: {0}", "Microphone/recognition unavailable: {0}";
    MageHeardNothing => "Hã? Não ouvi nada!", "Huh? I heard nothing!";
    Silence => "(silêncio)", "(silence)";
    FooterSilence => "Não ouvi nada. {0} ou clique no orbe para tentar de novo", "I heard nothing. {0} or click the orb to try again";
    Learned => "Aprendeu: {0}", "Learned: {0}";
    StageFirst => "{0} Primeira: {1}", "{0} First: {1}";
    NewItem => "{0} · Nova {1}: {2}", "{0} · New {1}: {2}";
    FooterNext => "{0}: próxima frase", "{0}: next phrase";
    AnswerCounted => "Resposta {0}! · ", "Answer: {0}! · ";
    RetryRecall => "Era assim: ouça e repita...", "It goes like this: listen and repeat...";
    RetryRepeat => "Ouça de novo...", "Listen again...";
    Attempt => "Tentativa {0}/{1}. {2}", "Try {0}/{1}. {2}";
    GiveUp => "Tudo bem, vamos praticar outra depois!", "That's ok, we'll practice another one later!";
    ComboSun => "COMBO x{0}! O SOL APARECEU!", "COMBO x{0}! THE SUN IS OUT!";
    GoalToast => "META DO DIA: {0} frases! Mandou bem!", "DAILY GOAL: {0} phrases! Well done!";
    GoalWarrior => "Meta do dia batida! Tô quentinho!", "Daily goal done! I'm nice and warm!";
    MageIdle => "Ninguém vai me enfrentar? Hahaha!", "Nobody dares to face me? Hahaha!";
    WarriorIdle => "Hora de praticar! Aperte {0}", "Time to practice! Press {0}";
    ToastIdle => "O mago está vencendo... {0} para enfrentar!", "The mage is winning... {0} to fight back!";
    RecapNone => "Hoje você ainda não praticou nenhuma frase. O guerreiro está com frio!",
        "You haven't practiced any phrase today. The warrior is cold!";
    RecapOne => "Resumo de hoje: você praticou uma frase.", "Today's recap: you practiced one phrase.";
    RecapMany => "Resumo de hoje: você praticou {0} frases.", "Today's recap: you practiced {0} phrases.";
    RecapTitle => "RESUMO DE HOJE · {0}", "TODAY'S RECAP · {0}";
    // chrono format of the day in the recap title.
    RecapDate => "%d/%m", "%b %d";
    RecapFooter => "{0} de {1} frases acertadas · meta {2}", "{0} of {1} phrases right · goal {2}";
    TopicDropped => "Sem {0} nesse nível.", "No {0} at this level.";
    // ---------------------------------------------------------------- caption
    StatusSpeaking => "OUÇA", "LISTEN";
    StatusListening => "FALE AGORA", "SPEAK NOW";
    StatusThinking => "PENSANDO", "THINKING";
    StatusPassed => "ACERTOU!", "RIGHT!";
    StatusFailed => "QUASE! TENTE DE NOVO", "ALMOST! TRY AGAIN";
    StatusConfirm => "FALE E CONFIRME", "SAY IT AND CONFIRM";
    Heard => "Ouvi: \"{0}\"", "I heard: \"{0}\"";
    Speaking => "falando", "speaking";
    // ---------------------------------------------------------------- app
    PauseOn => "Mago pausado. Bom foco!", "Mage paused. Stay focused!";
    PauseOff => "O mago voltou!", "The mage is back!";
    DeckUnavailable => "Deck indisponível: {0}", "Deck unavailable: {0}";
    CommitmentToast => "Compromisso: {0}", "Commitment: {0}";
    ModeRestart => "Modo de tela muda ao reiniciar o Snowlearner", "The screen mode changes when Snowlearner restarts";
    VoiceToast => "Voz: {0}", "Voice: {0}";
    NativeToast => "Minha língua: português · aprendendo {0}", "My language: English · learning {0}";
    Welcome => "Snowlearner · {0} · ajuda/painel: {1}", "Snowlearner · {0} · help/panel: {1}";
    CheatPractice => "praticar / terminei", "practice / done";
    CheatPanel => "painel", "panel";
    CheatRecap => "resumo do dia", "today's recap";
    CheatHand => "mão mágica", "magic hand";
    CheatProgress => "progresso", "progress";
    CheatFooter => "Orbe: clique pratica · Ctrl+clique painel · direito pausa",
        "Orb: click practices · Ctrl+click panel · right pauses";
    // ---------------------------------------------------------------- scene
    WarriorCheer => "Vai lá, você consegue!", "Go on, you can do it!";
    MageBlackHole => "Nããão! O buraco negro!", "Nooo! The black hole!";
    WarriorWarm => "Que calor bom! Valeu!", "Nice and warm! Thanks!";
    MageSun => "Aaah! O sol!!", "Aaah! The sun!!";
    WarriorSun => "Que solzão!", "What a sun!";
    WarriorHello => "Oi! Aperte o atalho e fale comigo!", "Hi! Press the hotkey and talk to me!";
    WarriorFire => "Brrr... vou acender uma fogueira!", "Brrr... I'll light a fire!";
    WarriorCold => "Ai! Que frio!", "Ow! So cold!";
    WarriorIce => "Ai! Gelado!", "Ow! Freezing!";
    WarriorMobs => "Monstros de gelo! Deixa comigo!", "Ice monsters! Leave them to me!";
    WarriorHit => "Toma!", "Take that!";
    WarriorIcicle => "Ai! Pingente!", "Ow! An icicle!";
    MageFriends => "Venham, amigos do gelo!", "Come, friends of the ice!";
    MageIcicles => "Chuva de gelo!", "Ice rain!";
    MageOwl => "Coruja de gelo, venha!", "Ice owl, come to me!";
    WarriorSlide => "Uhuu! Escorregando!", "Wheee! Sliding!";
    MageSlide => "Aaah! Escorrega!", "Aaah! Slippery!";
}

lines! {
    MageAskRepeat => ["Repita, se for capaz!", "Hah! Diga isso!", "Vamos ver essa pronúncia!"],
        ["Repeat it, if you can!", "Hah! Say that!", "Let's hear that accent!"];
    MageAskRecall => ["Duvido que lembre essa!", "Sem cola agora!", "Essa você já viu. E aí?"],
        ["Bet you don't remember this one!", "No peeking now!", "You've seen this one. Well?"];
    MageLaugh => ["Hahaha! Errou!", "Mais neve pra você!", "Quase... mas não!"],
        ["Hahaha! Wrong!", "More snow for you!", "Close... but no!"];
    MageGroan => ["Argh! Não!", "Impossível!", "Grrr... sorte!"],
        ["Argh! No!", "Impossible!", "Grrr... lucky!"];
    MagePoked => ["Ei! Não me cutuque!", "Mais neve pra você!", "Fale uma frase, se tiver coragem!"],
        ["Hey! Don't poke me!", "More snow for you!", "Say a phrase, if you dare!"];
    MageHeld => [
        "Me solta, aprendiz!",
        "Isso é humilhante!",
        "Eu sou um MAGO, não um brinquedo!",
        "Vou congelar sua tela inteira!",
        "Meu chapéu! Cuidado com o chapéu!",
    ], [
        "Let go of me, apprentice!",
        "This is humiliating!",
        "I am a MAGE, not a toy!",
        "I'll freeze your whole screen!",
        "My hat! Mind the hat!",
    ];
    MageLanded => ["Hmpf!", "Você vai pagar por isso!", "Ai... minha dignidade."],
        ["Hmph!", "You'll pay for this!", "Ow... my dignity."];
    WarriorHeld => [
        "Ei... isso é constrangedor... 👉👈",
        "Me coloca no chão, por favor...",
        "Todo mundo tá olhando... 👉👈",
        "Eu tenho medo de altura!",
        "N-não precisa me carregar... 👉👈",
        "S-senpai...? 👉👈",
    ], [
        "Hey... this is embarrassing... 👉👈",
        "Put me down, please...",
        "Everybody's looking... 👉👈",
        "I'm scared of heights!",
        "Y-you don't have to carry me... 👉👈",
        "S-senpai...? 👉👈",
    ];
    WarriorLanded => ["Ufa... obrigado? 👉👈", "Chão, doce chão.", "Não conta pra ninguém, tá?", "F-foi até legal... 👉👈"],
        ["Phew... thanks? 👉👈", "Ground, sweet ground.", "Don't tell anyone, ok?", "Th-that was kind of fun... 👉👈"];
    Help => [
        "TECLAS",
        "Espaço: praticar    S: resumo do dia",
        "M: painel           P: pausar o mago",
        "1-4: compromisso    L: trocar idioma",
        "H: esta ajuda       Esc: cancelar",
        "Orbe: clique = praticar · Ctrl+clique = painel",
        "      botão direito = pausar (buraco negro!)",
        "Clique no mago, no guerreiro, na fogueira...",
    ], [
        "KEYS",
        "Space: practice     S: today's recap",
        "M: panel            P: pause the mage",
        "1-4: commitment     L: switch language",
        "H: this help        Esc: cancel",
        "Orb: click = practice · Ctrl+click = panel",
        "     right click = pause (black hole!)",
        "Click the mage, the warrior, the campfire...",
    ];
}

impl T {
    /// The text with `{0}`, `{1}`… replaced by `args`, in order.
    pub fn fill(self, native: Native, args: &[&dyn std::fmt::Display]) -> String {
        let mut out = self.get(native).to_string();
        for (i, a) in args.iter().enumerate() {
            out = out.replace(&format!("{{{i}}}"), &a.to_string());
        }
        out
    }
}

/// A language's name in the learner's own words ("es" → "espanhol" / "Spanish").
/// Unknown codes (custom decks) stay as written.
pub fn language_name(native: Native, code: &str) -> String {
    let base = code.split(['-', '_']).next().unwrap_or(code).to_ascii_lowercase();
    let name = match (native, base.as_str()) {
        (Native::PtBr, "en") => "inglês",
        (Native::PtBr, "es") => "espanhol",
        (Native::PtBr, "pt") => "português",
        (Native::PtBr, "fr") => "francês",
        (Native::PtBr, "it") => "italiano",
        (Native::PtBr, "de") => "alemão",
        (Native::En, "en") => "English",
        (Native::En, "es") => "Spanish",
        (Native::En, "pt") => "Portuguese",
        (Native::En, "fr") => "French",
        (Native::En, "it") => "Italian",
        (Native::En, "de") => "German",
        _ => return code.to_string(),
    };
    name.to_string()
}

/// A Kokoro voice's language letter in the learner's words, with the accent's
/// region where one language has several: `a` → ("American English", "US").
pub fn kokoro_language(native: Native, letter: char) -> Option<(&'static str, Option<&'static str>)> {
    let pt = native == Native::PtBr;
    Some(match letter {
        'a' if pt => ("inglês americano", Some("EUA")),
        'a' => ("American English", Some("US")),
        'b' if pt => ("inglês britânico", Some("RU")),
        'b' => ("British English", Some("UK")),
        'e' => (if pt { "espanhol" } else { "Spanish" }, None),
        'f' => (if pt { "francês" } else { "French" }, None),
        'h' => (if pt { "hindi" } else { "Hindi" }, None),
        'i' => (if pt { "italiano" } else { "Italian" }, None),
        'j' => (if pt { "japonês" } else { "Japanese" }, None),
        'p' => (if pt { "português do Brasil" } else { "Brazilian Portuguese" }, None),
        'z' => (if pt { "chinês mandarim" } else { "Mandarin Chinese" }, None),
        _ => return None,
    })
}

/// Topic keys stay as the decks write them (pt-BR); this is how they read.
pub const TOPICS: &[(&str, &str)] = &[
    ("primeiros contatos", "first contact"),
    ("trabalho", "work"),
    ("restaurante", "restaurant"),
    ("viagem", "travel"),
    ("compras", "shopping"),
    ("saúde e social", "health and social"),
    ("comida e compras", "food and groceries"),
    ("rotina e transporte", "routine and transport"),
    ("emergência", "emergency"),
    ("saúde", "health"),
    ("social", "social"),
    ("telefone", "phone"),
    ("casa e burocracia", "home and paperwork"),
    ("gírias e conversa", "slang and small talk"),
    ("negociação e opinião", "negotiation and opinion"),
    ("vida no exterior", "life abroad"),
    ("geral", "general"),
];

/// A topic as the learner reads it; unknown topics (custom decks) as written.
pub fn topic(native: Native, key: &str) -> String {
    match native {
        Native::PtBr => key.to_string(),
        Native::En => TOPICS.iter().find(|(k, _)| *k == key).map_or(key, |(_, en)| en).to_string(),
    }
}

/// Ticked topics as one short label: "trabalho", "trabalho + viagem",
/// "3 temas". None when nothing is ticked (= every topic).
pub fn topics_label(native: Native, topics: &[String]) -> Option<String> {
    match topics {
        [] => None,
        [one] => Some(topic(native, one)),
        [a, b] => Some(format!("{} + {}", topic(native, a), topic(native, b))),
        many => Some(T::TopicsCount.fill(native, &[&many.len()])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::font;

    fn placeholders(s: &str) -> Vec<usize> {
        (0..10).filter(|i| s.contains(&format!("{{{i}}}"))).collect()
    }

    #[test]
    fn every_text_exists_in_both_languages_and_renders_with_the_pixel_font() {
        for t in T::ALL {
            let (pt, en) = (t.get(Native::PtBr), t.get(Native::En));
            assert!(!pt.trim().is_empty() && !en.trim().is_empty(), "{t:?} is empty");
            assert!(font::supports(pt), "{t:?} pt-BR: {pt:?}");
            assert!(font::supports(en), "{t:?} en: {en:?}");
            assert_eq!(placeholders(pt), placeholders(en), "{t:?}: both languages take the same values");
        }
    }

    #[test]
    fn every_line_set_has_lines_in_both_languages_and_renders() {
        for l in Lines::ALL {
            for n in Native::ALL {
                let lines = l.get(n);
                assert!(!lines.is_empty(), "{l:?} {n:?} has no lines");
                for s in lines {
                    assert!(font::supports(s), "{l:?} {n:?}: {s:?}");
                }
            }
        }
    }

    #[test]
    fn english_texts_are_not_left_in_portuguese() {
        // A key copied without translating would read the same in both.
        let same_ok = [T::ModeAuto, T::Today, T::PanelTitle, T::RecapDate];
        for t in T::ALL.iter().filter(|t| !same_ok.contains(t)) {
            if t.get(Native::PtBr).chars().filter(|c| c.is_alphabetic()).count() > 3 {
                assert_ne!(t.get(Native::PtBr), t.get(Native::En), "{t:?} is not translated");
            }
        }
    }

    #[test]
    fn fill_puts_values_in_order_and_leaves_the_rest() {
        assert_eq!(T::Attempt.fill(Native::En, &[&2, &3, &"Listen again..."]), "Try 2/3. Listen again...");
        assert_eq!(T::Attempt.fill(Native::PtBr, &[&1, &3, &"x"]), "Tentativa 1/3. x");
        assert_eq!(T::TipPrefix.fill(Native::En, &[&"{1} stays"]), "Tip: {1} stays");
    }

    #[test]
    fn languages_and_topics_read_in_the_learners_words() {
        assert_eq!(language_name(Native::PtBr, "es"), "espanhol");
        assert_eq!(language_name(Native::En, "pt-BR"), "Portuguese");
        assert_eq!(language_name(Native::En, "klingon"), "klingon");
        assert_eq!(topic(Native::En, "trabalho"), "work");
        assert_eq!(topic(Native::PtBr, "trabalho"), "trabalho");
        assert_eq!(topic(Native::En, "meu tema"), "meu tema");
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(topics_label(Native::En, &[]), None, "nothing ticked = all topics");
        assert_eq!(topics_label(Native::En, &t(&["trabalho"])).as_deref(), Some("work"));
        assert_eq!(topics_label(Native::PtBr, &t(&["trabalho", "viagem"])).as_deref(), Some("trabalho + viagem"));
        assert_eq!(topics_label(Native::En, &t(&["a", "b", "c"])).as_deref(), Some("3 topics"));
        for (k, en) in TOPICS {
            assert!(font::supports(k) && font::supports(en), "{k}");
        }
    }
}
