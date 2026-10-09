//! Reading more of the account and changing it: ratings, the library,
//! subscriptions, playlists, search suggestions and filters, a song's
//! details and its lyrics.
//!
//! The endpoints and request bodies follow ytmusicapi. Each body is built
//! by a small function of its own, tested below without YouTube; the
//! replies are read in [`crate::read`].

use serde_json::{Value, json};

use crate::innertube::{ApiError, LIBRARY_SONGS, Session, next_body};
use crate::lyrics::Lyrics;
use crate::read::{self, Continuation, Item, Page, Rating, SongDetails};
use crate::redact;

/// YouTube Music's Android app, which YouTube sends timed lyrics to; the
/// web client gets plain ones only. ytmusicapi's values: the web page has
/// no settings for another client.
const MOBILE_CLIENT_NAME: &str = "ANDROID_MUSIC";
const MOBILE_CLIENT_VERSION: &str = "7.21.50";
/// What YouTube answers when a playlist edit worked.
const SUCCEEDED: &str = "STATUS_SUCCEEDED";
const HISTORY: &str = "FEmusic_history";

/// Who can see a playlist.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Privacy {
    /// The account only.
    #[default]
    Private,
    /// Anyone with the link.
    Unlisted,
    /// Anyone; it can turn up in search.
    Public,
}

impl Privacy {
    /// All three, in YouTube Music's order.
    pub const ALL: [Self; 3] = [Self::Public, Self::Unlisted, Self::Private];

    /// Its name, and what it means, as YouTube Music's menu says them.
    pub fn words(self) -> (&'static str, &'static str) {
        match self {
            Self::Public => ("Public", "Anyone can search for and view"),
            Self::Unlisted => ("Unlisted", "Anyone with the link can view"),
            Self::Private => ("Private", "Only you can view"),
        }
    }

    /// Read from a playlist page's line ("Playlist • Public • 2024").
    pub fn from_subtitle(subtitle: &str) -> Option<Self> {
        subtitle
            .split('•')
            .map(str::trim)
            .find_map(|part| match part {
                "Public" => Some(Self::Public),
                "Unlisted" => Some(Self::Unlisted),
                "Private" => Some(Self::Private),
                _ => None,
            })
    }

    fn status(self) -> &'static str {
        match self {
            Self::Private => "PRIVATE",
            Self::Unlisted => "UNLISTED",
            Self::Public => "PUBLIC",
        }
    }
}

/// One kind of search result alone, as YouTube Music's buttons above the
/// results choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchFilter {
    Songs,
    Albums,
    Artists,
    /// Playlists made by listeners.
    Playlists,
    /// Playlists made by YouTube Music.
    FeaturedPlaylists,
}

impl SearchFilter {
    /// What tells YouTube which results to give (ytmusicapi's values).
    pub fn params(self) -> &'static str {
        match self {
            Self::Songs => "EgWKAQIIAWoMEA4QChADEAQQCRAF",
            Self::Albums => "EgWKAQIYAWoMEA4QChADEAQQCRAF",
            Self::Artists => "EgWKAQIgAWoMEA4QChADEAQQCRAF",
            Self::Playlists => "EgeKAQQoAEABagwQDhAKEAMQBBAJEAU%3D",
            Self::FeaturedPlaylists => "EgeKAQQoADgBagwQDhAKEAMQBBAJEAU%3D",
        }
    }
}

/// The parts of the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LibraryTab {
    /// The Library's front page: everything in it, most recently used
    /// first (YouTube Music's "Recent activity").
    Recent,
    Playlists,
    /// Songs saved to the library. A long list: [`Session::long_page`]
    /// with this tab's [`LibraryTab::browse_id`] loads all of it.
    Songs,
    Albums,
    /// The artists of the songs in the library.
    Artists,
    /// People's profiles the account follows (those with music).
    Profiles,
    /// Podcasts saved to the library.
    Podcasts,
    /// The artists the account subscribes to.
    Subscriptions,
}

impl LibraryTab {
    /// The tab's page.
    pub fn browse_id(self) -> &'static str {
        match self {
            Self::Recent => "FEmusic_library_landing",
            Self::Playlists => "FEmusic_liked_playlists",
            Self::Songs => LIBRARY_SONGS,
            Self::Albums => "FEmusic_liked_albums",
            Self::Artists => "FEmusic_library_corpus_track_artists",
            Self::Profiles => "FEmusic_library_user_profile_channels_list",
            Self::Podcasts => "FEmusic_library_non_music_audio_list",
            Self::Subscriptions => "FEmusic_library_corpus_artists",
        }
    }

    /// The `params` its page wants with its ID, as the Library's chip for
    /// it sends them (a sort order's own replace them).
    pub fn params(self) -> Option<&'static str> {
        match self {
            Self::Profiles => Some("ggMCCAc="),
            _ => None,
        }
    }
}

impl Session {
    /// Rates a song: a like also adds it to Liked Music; `Indifferent`
    /// takes the like or dislike away.
    pub async fn rate_song(&self, video_id: &str, rating: Rating) -> Result<(), ApiError> {
        self.call(like_endpoint(rating), song_target(video_id))
            .await?;
        Ok(())
    }

    /// Saves an album or a playlist to the library, or takes it out.
    /// `playlist_id` is the page header's `library_id` (for an album, its
    /// own playlist, `OLAK5uy_...`).
    pub async fn save_to_library(&self, playlist_id: &str, save: bool) -> Result<(), ApiError> {
        let rating = if save {
            Rating::Like
        } else {
            Rating::Indifferent
        };
        self.call(like_endpoint(rating), playlist_target(playlist_id))
            .await?;
        Ok(())
    }

    /// Subscribes to an artist (which puts them in the library), or
    /// unsubscribes. `channel_id` is the artist page header's
    /// `channel_id`.
    pub async fn subscribe(&self, channel_id: &str, subscribe: bool) -> Result<(), ApiError> {
        self.call(subscription_endpoint(subscribe), channels_body(channel_id))
            .await?;
        Ok(())
    }

    /// Makes a playlist, with `video_ids` in it (it can start empty).
    /// Returns its ID, without `VL`.
    pub async fn create_playlist(
        &self,
        title: &str,
        description: &str,
        privacy: Privacy,
        video_ids: &[String],
    ) -> Result<String, ApiError> {
        let body = create_playlist_body(title, description, privacy, video_ids);
        let reply = self.call("playlist/create", body).await?;
        read::created_playlist_id(&reply)
            .ok_or_else(|| ApiError::Unexpected("no ID for the new playlist".into()))
    }

    /// Deletes one of the account's own playlists.
    pub async fn delete_playlist(&self, playlist_id: &str) -> Result<(), ApiError> {
        let reply = self
            .call("playlist/delete", playlist_body(playlist_id))
            .await?;
        match read::edit_status(&reply) {
            Some(status) if status != SUCCEEDED => Err(refused(&status)),
            _ => Ok(()),
        }
    }

    /// Renames one of the account's own playlists.
    pub async fn rename_playlist(&self, playlist_id: &str, name: &str) -> Result<(), ApiError> {
        self.edit_playlist(edit_body(playlist_id, vec![rename_action(name)]))
            .await
    }

    /// Changes one of the account's own playlists' name, description and
    /// privacy at once (YouTube Music's Edit playlist).
    pub async fn edit_playlist_details(
        &self,
        playlist_id: &str,
        name: &str,
        description: &str,
        privacy: Privacy,
    ) -> Result<(), ApiError> {
        self.edit_playlist(edit_body(
            playlist_id,
            details_actions(name, description, privacy),
        ))
        .await
    }

    /// Adds songs to the end of one of the account's own playlists. A
    /// song already in it is added again (YouTube Music's "Add anyway").
    pub async fn add_to_playlist(
        &self,
        playlist_id: &str,
        video_ids: &[String],
    ) -> Result<(), ApiError> {
        if video_ids.is_empty() {
            return Ok(());
        }
        self.edit_playlist(edit_body(playlist_id, add_actions(video_ids)))
            .await
    }

    /// Takes rows out of one of the account's own playlists. Each item is
    /// a row's `(video_id, set_video_id)`: the row's own ID tells two
    /// copies of a song apart.
    pub async fn remove_from_playlist(
        &self,
        playlist_id: &str,
        items: &[(String, String)],
    ) -> Result<(), ApiError> {
        if items.is_empty() {
            return Ok(());
        }
        self.edit_playlist(edit_body(playlist_id, remove_actions(items)))
            .await
    }

    async fn edit_playlist(&self, body: Value) -> Result<(), ApiError> {
        let reply = self.call("browse/edit_playlist", body).await?;
        match read::edit_status(&reply) {
            Some(status) if status == SUCCEEDED => Ok(()),
            status => Err(refused(status.as_deref().unwrap_or("no answer"))),
        }
    }

    /// What to suggest while `text` is typed into search: past searches
    /// and YouTube Music's suggestions. With nothing typed yet, the past
    /// searches.
    pub async fn search_suggestions(&self, text: &str) -> Result<read::Suggestions, ApiError> {
        let reply = self
            .call("music/get_search_suggestions", suggestions_body(text))
            .await?;
        Ok(read::search_suggestions(&reply))
    }

    /// Removes one of the account's past searches from its search
    /// history, by the token its suggestion carries
    /// ([`read::SuggestedWords::forget`]).
    pub async fn forget_search(&self, token: &str) -> Result<(), ApiError> {
        let reply = self.call("feedback", feedback_body(token)).await?;
        match read::feedback_processed(&reply) {
            Some(false) => Err(ApiError::Unexpected(
                "the search was not removed from the history".into(),
            )),
            _ => Ok(()),
        }
    }

    /// Search results of one kind only (songs, albums...): `params` as a
    /// filter button gives them ([`SearchFilter`] has some). With them,
    /// where the next ones come from, asked for only when the list's end
    /// is reached ([`Session::more_search_results`]), as YouTube Music
    /// does.
    pub async fn search_filtered(
        &self,
        query: &str,
        params: &str,
    ) -> Result<(Page, Option<Continuation>), ApiError> {
        let reply = self.call("search", search_body(query, params)).await?;
        Ok((read::page(&reply), read::item_continuation(&reply)))
    }

    /// The next results of a search of one kind, and where the ones after
    /// them come from (`None` at the end).
    pub async fn more_search_results(
        &self,
        query: &str,
        params: &str,
        from: &Continuation,
    ) -> Result<(Vec<Item>, Option<Continuation>), ApiError> {
        let reply = match from {
            Continuation::Body(token) => {
                self.call("search", json!({ "continuation": token }))
                    .await?
            }
            // As ytmusicapi sends it: the search again, the token in the
            // address.
            Continuation::Address(token) => {
                let query_args = [("ctoken", token.as_str()), ("continuation", token)];
                self.call_with("search", &query_args, search_body(query, params))
                    .await?
            }
        };
        Ok((read::more_items(&reply), read::item_continuation(&reply)))
    }

    /// Whether the account likes a song, and where its lyrics and related
    /// songs are. Asks for the same thing as [`Session::up_next`].
    pub async fn song_details(
        &self,
        video_id: &str,
        playlist_id: Option<&str>,
    ) -> Result<SongDetails, ApiError> {
        let reply = self.call("next", next_body(video_id, playlist_id)).await?;
        Ok(read::song_details(&reply))
    }

    /// A song's lyrics from YouTube Music, by the lyrics page in its
    /// [`SongDetails`]. Asks as YouTube Music's Android app first, which
    /// gets timed lyrics where there are some; otherwise (or when that
    /// fails) as the web client, for plain ones. `Ok(None)` when YouTube
    /// has none.
    pub async fn lyrics(&self, browse_id: &str) -> Result<Option<Lyrics>, ApiError> {
        let body = browse_body(browse_id);
        let mobile = self
            .call_as(
                "browse",
                body.clone(),
                MOBILE_CLIENT_NAME,
                MOBILE_CLIENT_VERSION,
            )
            .await;
        match mobile {
            // Timed lyrics, or plain ones: the web client has no more.
            Ok(reply) => {
                if let Some(lyrics) = read::lyrics(&reply) {
                    return Ok(Some(lyrics));
                }
            }
            Err(e) => log::info!("timed lyrics could not be fetched: {e}"),
        }
        let reply = self.call("browse", body).await?;
        Ok(read::lyrics(&reply))
    }

    /// One part of the library, with the rest of a long list up to a
    /// limit (see `Session::library_page`), in the order `params` asks for
    /// (one of its sort button's, [`read::SortOrder`]), else YouTube's.
    /// The whole of the library's songs comes from [`Session::long_page`]
    /// instead.
    pub async fn library(&self, tab: LibraryTab, params: Option<&str>) -> Result<Page, ApiError> {
        self.library_page(tab.browse_id(), params.or(tab.params()))
            .await
    }

    /// Listening History as a page, grouped by when (Today, Yesterday...).
    pub async fn history_page(&self) -> Result<Page, ApiError> {
        self.page(HISTORY, None).await
    }
}

fn like_endpoint(rating: Rating) -> &'static str {
    match rating {
        Rating::Like => "like/like",
        Rating::Dislike => "like/dislike",
        Rating::Indifferent => "like/removelike",
    }
}

fn song_target(video_id: &str) -> Value {
    json!({ "target": { "videoId": video_id } })
}

fn playlist_target(playlist_id: &str) -> Value {
    json!({ "target": { "playlistId": bare(playlist_id) } })
}

fn subscription_endpoint(subscribe: bool) -> &'static str {
    if subscribe {
        "subscription/subscribe"
    } else {
        "subscription/unsubscribe"
    }
}

fn channels_body(channel_id: &str) -> Value {
    json!({ "channelIds": [channel_id] })
}

fn create_playlist_body(
    title: &str,
    description: &str,
    privacy: Privacy,
    video_ids: &[String],
) -> Value {
    let mut body = json!({
        "title": clean(title),
        "description": clean(description),
        "privacyStatus": privacy.status(),
    });
    if !video_ids.is_empty() {
        body["videoIds"] = json!(video_ids);
    }
    body
}

fn playlist_body(playlist_id: &str) -> Value {
    json!({ "playlistId": bare(playlist_id) })
}

fn edit_body(playlist_id: &str, actions: Vec<Value>) -> Value {
    json!({ "playlistId": bare(playlist_id), "actions": actions })
}

fn rename_action(name: &str) -> Value {
    json!({ "action": "ACTION_SET_PLAYLIST_NAME", "playlistName": clean(name) })
}

/// A name, a description and a privacy, as ytmusicapi's `edit_playlist`
/// sends them.
fn details_actions(name: &str, description: &str, privacy: Privacy) -> Vec<Value> {
    vec![
        rename_action(name),
        json!({ "action": "ACTION_SET_PLAYLIST_DESCRIPTION", "playlistDescription": clean(description) }),
        json!({ "action": "ACTION_SET_PLAYLIST_PRIVACY", "playlistPrivacy": privacy.status() }),
    ]
}

fn add_actions(video_ids: &[String]) -> Vec<Value> {
    video_ids
        .iter()
        .map(|id| {
            json!({
                "action": "ACTION_ADD_VIDEO",
                "addedVideoId": id,
                // Add even when the song is already in the playlist,
                // rather than refuse the whole edit.
                "dedupeOption": "DEDUPE_OPTION_SKIP",
            })
        })
        .collect()
}

fn remove_actions(items: &[(String, String)]) -> Vec<Value> {
    items
        .iter()
        .map(|(video_id, set_video_id)| {
            json!({
                "action": "ACTION_REMOVE_VIDEO",
                "setVideoId": set_video_id,
                "removedVideoId": video_id,
            })
        })
        .collect()
}

fn suggestions_body(text: &str) -> Value {
    json!({ "input": text })
}

fn feedback_body(token: &str) -> Value {
    json!({ "feedbackTokens": [token] })
}

fn search_body(query: &str, params: &str) -> Value {
    json!({ "query": query, "params": params })
}

fn browse_body(browse_id: &str) -> Value {
    json!({ "browseId": browse_id })
}

/// A playlist's own ID: its page's ID without `VL`.
fn bare(playlist_id: &str) -> &str {
    playlist_id.strip_prefix("VL").unwrap_or(playlist_id)
}

/// YouTube Music breaks on `<` and `>` in a playlist's name or
/// description (ytmusicapi), so they are left out.
fn clean(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|c| !matches!(c, '<' | '>'))
        .collect()
}

fn refused(status: &str) -> ApiError {
    ApiError::Unexpected(format!(
        "the playlist was not changed ({})",
        redact::urls(status)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rating_and_saving() {
        assert_eq!(like_endpoint(Rating::Like), "like/like");
        assert_eq!(like_endpoint(Rating::Dislike), "like/dislike");
        assert_eq!(like_endpoint(Rating::Indifferent), "like/removelike");
        assert_eq!(
            song_target("abcdefghijk"),
            json!({"target": {"videoId": "abcdefghijk"}})
        );
        assert_eq!(
            playlist_target("OLAK5uy_album"),
            json!({"target": {"playlistId": "OLAK5uy_album"}})
        );
        assert_eq!(
            playlist_target("VLPLmine"),
            json!({"target": {"playlistId": "PLmine"}})
        );
    }

    #[test]
    fn subscribing() {
        assert_eq!(subscription_endpoint(true), "subscription/subscribe");
        assert_eq!(subscription_endpoint(false), "subscription/unsubscribe");
        assert_eq!(
            channels_body("UCartist"),
            json!({"channelIds": ["UCartist"]})
        );
    }

    #[test]
    fn making_and_deleting_playlists() {
        assert_eq!(
            create_playlist_body(" Road <trip> ", "For the car", Privacy::Unlisted, &[]),
            json!({
                "title": "Road trip",
                "description": "For the car",
                "privacyStatus": "UNLISTED"
            })
        );
        let songs = ["song0000001".to_string(), "song0000002".to_string()];
        let body = create_playlist_body("Mix", "", Privacy::default(), &songs);
        assert_eq!(body["privacyStatus"], "PRIVATE");
        assert_eq!(body["videoIds"], json!(["song0000001", "song0000002"]));
        assert_eq!(Privacy::Public.status(), "PUBLIC");
        assert_eq!(playlist_body("VLPLmine"), json!({"playlistId": "PLmine"}));
    }

    #[test]
    fn editing_a_playlists_details() {
        assert_eq!(
            details_actions(" Road <trip> ", "For the car", Privacy::Unlisted),
            [
                json!({"action": "ACTION_SET_PLAYLIST_NAME", "playlistName": "Road trip"}),
                json!({"action": "ACTION_SET_PLAYLIST_DESCRIPTION", "playlistDescription": "For the car"}),
                json!({"action": "ACTION_SET_PLAYLIST_PRIVACY", "playlistPrivacy": "UNLISTED"}),
            ]
        );
        assert_eq!(
            Privacy::from_subtitle("Playlist • Public • 2024"),
            Some(Privacy::Public)
        );
        assert_eq!(Privacy::from_subtitle("Auto playlist • 2026"), None);
    }

    #[test]
    fn editing_playlists() {
        assert_eq!(
            edit_body("VLPLmine", vec![rename_action("New name")]),
            json!({
                "playlistId": "PLmine",
                "actions": [{"action": "ACTION_SET_PLAYLIST_NAME", "playlistName": "New name"}]
            })
        );
        assert_eq!(
            add_actions(&["song0000001".to_string()]),
            [json!({
                "action": "ACTION_ADD_VIDEO",
                "addedVideoId": "song0000001",
                "dedupeOption": "DEDUPE_OPTION_SKIP"
            })]
        );
        // The same song twice: each row by its own ID.
        let rows = [
            ("song0000001".to_string(), "ROWA".to_string()),
            ("song0000001".to_string(), "ROWB".to_string()),
        ];
        assert_eq!(
            remove_actions(&rows),
            [
                json!({"action": "ACTION_REMOVE_VIDEO", "setVideoId": "ROWA", "removedVideoId": "song0000001"}),
                json!({"action": "ACTION_REMOVE_VIDEO", "setVideoId": "ROWB", "removedVideoId": "song0000001"}),
            ]
        );
        assert!(
            refused("STATUS_FAILED")
                .to_string()
                .contains("STATUS_FAILED")
        );
    }

    #[test]
    fn searching() {
        assert_eq!(suggestions_body("fad"), json!({"input": "fad"}));
        // Nothing typed yet: the past searches.
        assert_eq!(suggestions_body(""), json!({"input": ""}));
        assert_eq!(feedback_body("t"), json!({"feedbackTokens": ["t"]}));
        assert_eq!(
            search_body("daft punk", SearchFilter::Songs.params()),
            json!({"query": "daft punk", "params": "EgWKAQIIAWoMEA4QChADEAQQCRAF"})
        );
        assert_eq!(
            SearchFilter::Albums.params(),
            "EgWKAQIYAWoMEA4QChADEAQQCRAF"
        );
        assert_eq!(
            SearchFilter::Artists.params(),
            "EgWKAQIgAWoMEA4QChADEAQQCRAF"
        );
        assert_eq!(
            SearchFilter::Playlists.params(),
            "EgeKAQQoAEABagwQDhAKEAMQBBAJEAU%3D"
        );
        assert_eq!(
            SearchFilter::FeaturedPlaylists.params(),
            "EgeKAQQoADgBagwQDhAKEAMQBBAJEAU%3D"
        );
    }

    #[test]
    fn library_pages() {
        assert_eq!(browse_body("MPLYt_x"), json!({"browseId": "MPLYt_x"}));
        let ids: Vec<&str> = [
            LibraryTab::Playlists,
            LibraryTab::Songs,
            LibraryTab::Albums,
            LibraryTab::Artists,
            LibraryTab::Profiles,
            LibraryTab::Podcasts,
            LibraryTab::Subscriptions,
        ]
        .iter()
        .map(|t| t.browse_id())
        .collect();
        assert_eq!(
            ids,
            [
                "FEmusic_liked_playlists",
                "FEmusic_liked_videos",
                "FEmusic_liked_albums",
                "FEmusic_library_corpus_track_artists",
                "FEmusic_library_user_profile_channels_list",
                "FEmusic_library_non_music_audio_list",
                "FEmusic_library_corpus_artists"
            ]
        );
        // Profiles' chip sends params of its own (measured signed in).
        assert_eq!(LibraryTab::Profiles.params(), Some("ggMCCAc="));
        assert_eq!(LibraryTab::Podcasts.params(), None);
    }
}
