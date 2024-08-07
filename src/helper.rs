use regex::Regex;
use serde_json::Value;
use url::Url;

use crate::{channel_info::{channel_video::ChannelVideo, ChannelInfo}, crate_client, playlist_info::{playlist_video::PlaylistVideo, PlaylistInfo}};

pub(crate) fn initial_data(watch_html: &str) -> Option<String> {
    let regex_pattern = vec![
        r#"window\[['"]ytInitialData['"]]\s*=\s*"#,
        r#"ytInitialData\s*=\s*"#,
    ];
    for pattern in regex_pattern {
        let reg_p =  Regex::new(pattern).unwrap();
        if let Some(initial_data) = reg_p.find(&watch_html) {
            let start_point = initial_data.end();
            let object: String = find_object_from_startpoint(&watch_html, start_point)?;
            return Some(object);
        }
    }
    None
}

pub(crate) fn find_object_from_startpoint(html: &str, start_point: usize) -> Option<String> {
    let html = &html[start_point..];
    let chars: Vec<char> = html.chars().collect();

    if !matches!(chars.get(0), Some('{') | Some('[')) {
        return None;
    }

    let mut last_char = '{';
    let mut curr_char: Option<char> = None;
    let mut stack = vec![chars[0]];
    let mut i = 1;

    let context_closers = [
        ('{', '}'),
        ('[', ']'),
        ('"', '"'),
        ('/', '/'), // JavaScript regex
    ].iter().cloned().collect::<std::collections::HashMap<_, _>>();

    while i < chars.len() {
        if stack.is_empty() {
            break;
        }

        if let Some(curr) = curr_char {
            if !curr.is_whitespace() {
                last_char = curr;
            }
        }

        curr_char = Some(chars[i]);
        let curr_context = stack.last().unwrap();

        if let Some(curr_char) = curr_char {
            if curr_char == context_closers[curr_context] {
                stack.pop();
                i += 1;
                continue;
            }

            if *curr_context == '"' || *curr_context == '/' {
                if curr_char == '\\' {
                    i += 2;
                    continue;
                }
            } else {
                if context_closers.contains_key(&curr_char) {
                    if !(curr_char == '/' && !matches!(last_char, '(' | ',' | '=' | ':' | '[' | '!' | '&' | '|' | '?' | '{' | '}' | ';')) {
                        stack.push(curr_char);
                    }
                }
            }
        }

        i += 1;
    }

    let full_obj: String = chars.iter().take(i).collect();
    Some(full_obj)
}

pub(crate) fn parese_playlist_videos(obj_data: &str) -> (Vec<PlaylistVideo>, Option<String>) {
    let initial_data: Value = serde_json::from_str(&obj_data).unwrap();
    let videos;
    let section_contents = initial_data["contents"][
        "twoColumnBrowseResultsRenderer"][
        "tabs"][0]["tabRenderer"]["content"][
        "sectionListRenderer"]["contents"].clone();
    if !section_contents.is_null() {
        let mut important_content = section_contents[
            0]["itemSectionRenderer"][
            "contents"][0]["playlistVideoListRenderer"].clone();
        if important_content.is_null() {
            important_content = section_contents[
                    1]["itemSectionRenderer"][
                    "contents"][0]["playlistVideoListRenderer"].clone()
        }
        videos = important_content["contents"].clone();
    } else {
        let important_content = initial_data["onResponseReceivedActions"][0]["appendContinuationItemsAction"]["continuationItems"].clone();
        videos = important_content;
    }
    let mut videos_raw = videos.as_array().unwrap().to_owned();
    let mut results = Vec::new();
    let continuation = videos_raw[videos_raw.len() - 1]["continuationItemRenderer"]["continuationEndpoint"]["continuationCommand"]["token"].clone();
    let mut continuation_id = None;
    if let Some(continuation) = continuation.as_str() {
        let continuation_index = videos_raw.len() - 1;
        videos_raw = videos_raw[..continuation_index].to_vec();
        continuation_id = Some(continuation.to_string());
    }
    for video in videos_raw {     
        let pvideo = serde_json::from_value::<PlaylistVideo>(video["playlistVideoRenderer"].clone()).unwrap();   
        results.push(pvideo);
    }
    
    (results, continuation_id)
}

pub(crate) fn parese_playlist_metadata(obj_data: &str) -> Result<PlaylistInfo, crate::Error> {
    let initial_data: Value = serde_json::from_str(&obj_data)?;
    let playlist_info_v = initial_data["microformat"]["microformatDataRenderer"].clone();
    let playlist_info: PlaylistInfo = serde_json::from_value(playlist_info_v).unwrap();
    Ok(playlist_info)
}

pub(crate) fn parese_channel_videos(obj_data: &str, channel_name: String) -> (Vec<ChannelVideo>, Option<String>) {
    let initial_data: Value = serde_json::from_str(&obj_data).unwrap();
    let mut videos = initial_data["contents"][
        "twoColumnBrowseResultsRenderer"][
        "tabs"][1]["tabRenderer"]["content"]["richGridRenderer"]["contents"].clone();
    if videos.is_null() {
        videos = initial_data["onResponseReceivedActions"][0]["appendContinuationItemsAction"]["continuationItems"].clone();
    }
    if videos.is_null() {
        return (Vec::new(), None);
    }
    let mut videos_raw = videos.as_array().unwrap().to_owned();
    let mut results = Vec::new();
    let continuation = videos_raw[videos_raw.len() - 1]["continuationItemRenderer"]["continuationEndpoint"]["continuationCommand"]["token"].clone();
    let mut continuation_id = None;
    if let Some(continuation) = continuation.as_str() {
        let continuation_index = videos_raw.len() - 1;
        videos_raw = videos_raw[..continuation_index].to_vec();
        continuation_id = Some(continuation.to_string());
    }
    for video in videos_raw {
        let pvideo = serde_json::from_value::<ChannelVideo>(video["richItemRenderer"]["content"]["videoRenderer"].clone());
        if pvideo.is_err() { 
            continue; 
        }
        results.push(pvideo.unwrap().add_author(channel_name.clone()));
    }
    
    (results, continuation_id)
}

pub(crate) fn parese_channel_metadata(obj_data: &str) -> Result<ChannelInfo, crate::Error> {
    let initial_data: Value = serde_json::from_str(&obj_data)?;
    let playlist_info_v = initial_data["microformat"]["microformatDataRenderer"].clone();
    let playlist_info: ChannelInfo = serde_json::from_value(playlist_info_v).unwrap();
    Ok(playlist_info)
}

pub async fn search_videos(query: &str) -> crate::Result<Vec<ChannelVideo>> {
    let client = crate_client().unwrap();
    let mut req_url = Url::parse("https://www.youtube.com/results").unwrap();
    req_url.query_pairs_mut().append_pair("search_query", query);
    let req = client.get(req_url).send().await?;
    let body = req.text().await?;
    let init_str = initial_data(&body).unwrap();
    let init_obj: Value = serde_json::from_str(&init_str).unwrap();
    let mut videos = Vec::new();
    let root_data = init_obj["contents"]["twoColumnSearchResultsRenderer"]["primaryContents"]["sectionListRenderer"]["contents"][0].clone();
    let mut videos_raw = root_data["itemSectionRenderer"]["contents"][1]["shelfRenderer"]["content"]["verticalListRenderer"]["items"].clone();
    if videos_raw.is_null() {
        videos_raw = root_data["itemSectionRenderer"]["contents"].clone();
    }
    let video_raw_vec = videos_raw.as_array().unwrap();
    for video_raw in video_raw_vec {
        if video_raw["videoRenderer"].is_null() {
            continue;
        }
        let pvideo = serde_json::from_value::<ChannelVideo>(video_raw["videoRenderer"].clone());
        if pvideo.is_err() {
            continue;
        }
        let mut video = pvideo.unwrap();
        if video.author.is_empty() {
            video.add_author(video_raw["videoRenderer"]["ownerText"]["runs"][0]["text"].as_str().unwrap().to_string());
        }
        videos.push(video);
    }
    Ok(videos)
}