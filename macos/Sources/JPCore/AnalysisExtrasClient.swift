import Foundation

extension CorpusClient {
    public func ngrams(_ query: NgramQuery) async throws -> NgramResult {
        try await perform { try $0.call("ngrams", ["query": AnyEncodable(query)]) }
    }

    public func inflection(_ query: InflectionQuery) async throws -> InflectionResult {
        try await perform { try $0.call("inflection", ["query": AnyEncodable(query)]) }
    }

    public func orthography(_ query: OrthographyQuery) async throws -> OrthographyResult {
        try await perform { try $0.call("orthography", ["query": AnyEncodable(query)]) }
    }

    public func cqlSearch(_ query: CqlQuery) async throws -> [KwicHit] {
        try await perform { try $0.call("cqlSearch", ["query": AnyEncodable(query)]) }
    }

    public func kwicBreakdown(_ query: BreakdownQuery) async throws -> BreakdownResult {
        try await perform { try $0.call("kwicBreakdown", ["query": AnyEncodable(query)]) }
    }

    public func collocationNetwork(_ query: NetworkQuery) async throws -> NetworkResult {
        try await perform { try $0.call("collocationNetwork", ["query": AnyEncodable(query)]) }
    }
}
