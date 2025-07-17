#[cfg(test)]
mod tests {
    use crate::SeekableHttpStream;
    use std::io::{Read, Seek, SeekFrom};
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_seekable_http_stream_mock_server() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mock_server = rt.block_on(MockServer::start());
        let data = b"Hello wiremock!";

        let len_header = data.len().to_string();
        rt.block_on(async {
            Mock::given(method("HEAD"))
                .respond_with(
                    ResponseTemplate::new(200).insert_header("Content-Length", len_header.as_str()),
                )
                .expect(1)
                .mount(&mock_server)
                .await;

            let range_header = format!("bytes=0-{}", data.len() - 1);
            Mock::given(method("GET"))
                .and(header("Range", range_header.as_str()))
                .respond_with(ResponseTemplate::new(206).set_body_bytes(data))
                .expect(2)
                .mount(&mock_server)
                .await;
        });

        let url = format!("{}/file.mp4", mock_server.uri());
        let mut stream = SeekableHttpStream::new(url).unwrap();

        let mut buf = [0u8; 5];
        stream.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, &data[0..5]);

        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, data[5..].to_vec());

        stream.seek(SeekFrom::Start(0)).unwrap();
        let mut all = Vec::new();
        stream.read_to_end(&mut all).unwrap();
        assert_eq!(all, data);

        assert_eq!(stream.http_request_count(), 3);
    }
}
