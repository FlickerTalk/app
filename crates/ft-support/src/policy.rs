//! What the support contact says (in English, for now): a few canned replies chosen by what came
//! in. It never repeats the user's words back, nor anything else of theirs but how long a text was;
//! every reply makes clear that a program answers.

/// Where a person answers.
pub const SUPPORT_LINK: &str = "https://flickertalk.com/support";

/// Said to whoever calls while another call goes on.
pub const BUSY: &str = "I am on another call right now. Please try again in a couple of minutes.";

/// Said to whoever calls when calls cannot be answered here.
pub const CALLS_OFF: &str = "This automated contact cannot take calls right now. A person answers at https://flickertalk.com/support.";

/// Said once a day to whoever used up the day's automated replies.
pub const DAILY_CAP: &str = "That is all the automated replies for today. A person answers at https://flickertalk.com/support.";

const WELCOME: &str = "Hi! This is the FlickerTalk support contact. Replies here are automatic: a program answers, not a person. \
Your message arrived end-to-end encrypted. Try it out: send a photo, a file or a voice message and one comes back, or call with \
voice or video and a recorded message answers. For help from a person, see https://flickertalk.com/support. Report and Block are \
in this contact's page.";

const FILE_BACK: &str = "Your file arrived, end-to-end encrypted. Here is a sample file back.";

const VOICE_BACK: &str = "Your voice message arrived. Here is one back.";

/// One message that came in, as much as the policy needs of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    Text(String),
    /// A file whose type is audio: what the app sends as a voice message.
    Voice,
    File,
}

impl Incoming {
    /// A file, by its MIME type.
    pub fn file(mime: &str) -> Self {
        if mime.trim().to_ascii_lowercase().starts_with("audio/") {
            Self::Voice
        } else {
            Self::File
        }
    }
}

/// What a text is about, by a few words in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Topic {
    Calls,
    Help,
    Other,
}

impl Topic {
    fn of(text: &str) -> Self {
        let text = text.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|word| text.contains(word));
        if has(&["help", "support", "problem", "bug", "bother", "abuse", "spam"]) {
            Self::Help
        } else if has(&["call", "video", "camera", "phone"]) {
            Self::Calls
        } else {
            Self::Other
        }
    }
}

/// "1 character", "27 characters".
fn characters(text: &str) -> String {
    match text.chars().count() {
        1 => "1 character".to_owned(),
        count => format!("{count} characters"),
    }
}

/// The answer to a text that is not the first: its length and a pointer that fits what it is about.
fn acknowledgement(text: &str) -> String {
    let got = format!("Got your message ({}), end-to-end encrypted. This is an automated reply.", characters(text));
    let more = match Topic::of(text) {
        Topic::Calls => "To try a call, tap the phone or the camera button in this chat: a recorded message answers, for up to two minutes. Nothing of the call is recorded.".to_owned(),
        Topic::Help => format!("A program cannot solve problems, but a person answers at {SUPPORT_LINK}. If someone bothers you, use Report or Block in their contact page."),
        Topic::Other => format!("For help from a person, see {SUPPORT_LINK}."),
    };
    format!("{got} {more}")
}

/// What the support contact does about a batch of messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// A 👍 on the first message of the batch.
    ThumbsUp,
    Text(String),
    /// The bundled sample file.
    SampleFile,
    /// The bundled voice message.
    VoiceMessage,
}

/// The replies to what one contact sent since the last answer. `first`: nothing of theirs was
/// ever answered.
pub fn replies(first: bool, batch: &[Incoming]) -> Vec<Reply> {
    if batch.is_empty() {
        return Vec::new();
    }
    let mut replies = Vec::new();
    let last_text = batch.iter().rev().find_map(|incoming| if let Incoming::Text(text) = incoming { Some(text) } else { None });
    if first {
        replies.push(Reply::ThumbsUp);
        replies.push(Reply::Text(WELCOME.to_owned()));
    } else if let Some(text) = last_text {
        replies.push(Reply::Text(acknowledgement(text)));
    }
    let mut files = Vec::new();
    if batch.contains(&Incoming::Voice) {
        replies.push(Reply::Text(VOICE_BACK.to_owned()));
        files.push(Reply::VoiceMessage);
    }
    if batch.contains(&Incoming::File) {
        replies.push(Reply::Text(FILE_BACK.to_owned()));
        files.push(Reply::SampleFile);
    }
    replies.extend(files);
    replies
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(words: &str) -> Incoming {
        Incoming::Text(words.to_owned())
    }

    fn texts_of(replies: &[Reply]) -> Vec<&str> {
        replies.iter().filter_map(|reply| if let Reply::Text(text) = reply { Some(text.as_str()) } else { None }).collect()
    }

    #[test]
    fn nothing_new_gets_nothing() {
        assert!(replies(true, &[]).is_empty());
        assert!(replies(false, &[]).is_empty());
    }

    #[test]
    fn the_first_message_gets_a_thumbs_up_and_the_welcome() {
        let answer = replies(true, &[text("hello")]);
        assert_eq!(answer[0], Reply::ThumbsUp);
        let said = texts_of(&answer);
        assert_eq!(said.len(), 1, "one welcome, no acknowledgement on top");
        assert!(said[0].contains("automatic"), "it says a program answers");
        assert!(said[0].contains(SUPPORT_LINK));
        assert!(said[0].contains("Report") && said[0].contains("Block"));
    }

    #[test]
    fn a_later_text_is_acknowledged_with_its_length() {
        let said = replies(false, &[text("where are my messages kept?")]);
        assert_eq!(said.len(), 1);
        let Reply::Text(answer) = &said[0] else { panic!("a text") };
        assert!(answer.contains("27 characters"), "{answer}");
        assert!(answer.contains("automated"));
        assert!(answer.contains(SUPPORT_LINK));
    }

    #[test]
    fn length_counts_characters_not_bytes_and_one_is_singular() {
        let Reply::Text(answer) = &replies(false, &[text("👋")])[0] else { panic!("a text") };
        assert!(answer.contains("(1 character)"), "{answer}");
    }

    #[test]
    fn several_texts_at_once_get_one_answer_about_the_last() {
        let said = replies(false, &[text("one"), text("two"), text("three!")]);
        assert_eq!(said.len(), 1);
        let Reply::Text(answer) = &said[0] else { panic!("a text") };
        assert!(answer.contains("6 characters"), "{answer}");
    }

    #[test]
    fn asking_about_calls_points_to_the_call_buttons() {
        let Reply::Text(answer) = &replies(false, &[text("Can I make a VIDEO call?")])[0] else { panic!("a text") };
        assert!(answer.contains("call"), "{answer}");
        assert!(answer.contains("camera button"), "{answer}");
    }

    #[test]
    fn asking_for_help_points_to_a_person_and_to_report_and_block() {
        let Reply::Text(answer) = &replies(false, &[text("I need help, someone is bothering me")])[0] else { panic!("a text") };
        assert!(answer.contains(SUPPORT_LINK));
        assert!(answer.contains("Report") && answer.contains("Block"), "{answer}");
    }

    #[test]
    fn a_voice_message_gets_one_back_and_a_file_gets_the_sample() {
        assert_eq!(replies(false, &[Incoming::Voice]), vec![Reply::Text(VOICE_BACK.to_owned()), Reply::VoiceMessage]);
        assert_eq!(replies(false, &[Incoming::File]), vec![Reply::Text(FILE_BACK.to_owned()), Reply::SampleFile]);
    }

    #[test]
    fn a_batch_with_both_kinds_and_a_text_gets_each_once() {
        let answer = replies(false, &[Incoming::File, text("look"), Incoming::Voice, Incoming::File]);
        assert_eq!(answer.iter().filter(|reply| **reply == Reply::SampleFile).count(), 1);
        assert_eq!(answer.iter().filter(|reply| **reply == Reply::VoiceMessage).count(), 1);
        assert_eq!(texts_of(&answer).len(), 3, "the acknowledgement and one line per kind");
        assert!(matches!(answer[0], Reply::Text(_)), "the words come first, then the files");
    }

    #[test]
    fn a_first_file_gets_the_welcome_and_the_sample() {
        let answer = replies(true, &[Incoming::File]);
        assert_eq!(answer[0], Reply::ThumbsUp);
        assert!(answer.contains(&Reply::SampleFile));
        assert!(texts_of(&answer)[0].contains("automatic"));
    }

    #[test]
    fn no_reply_repeats_the_user_s_words() {
        let secret = "my-bank-pin-4711";
        for first in [true, false] {
            for said in texts_of(&replies(first, &[text(secret), text(&format!("help {secret}"))])) {
                assert!(!said.contains(secret), "{said}");
            }
        }
    }

    #[test]
    fn audio_is_a_voice_message_and_anything_else_a_file() {
        assert_eq!(Incoming::file("audio/mp4"), Incoming::Voice);
        assert_eq!(Incoming::file("audio/webm"), Incoming::Voice);
        assert_eq!(Incoming::file("image/png"), Incoming::File);
        assert_eq!(Incoming::file("application/pdf"), Incoming::File);
        assert_eq!(Incoming::file(""), Incoming::File);
    }

    #[test]
    fn the_fixed_notices_point_to_a_person() {
        for notice in [CALLS_OFF, DAILY_CAP] {
            assert!(notice.contains(SUPPORT_LINK), "{notice}");
        }
        assert!(!BUSY.is_empty());
    }
}
