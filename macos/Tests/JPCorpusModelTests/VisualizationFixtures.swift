import Foundation
import JPCore

enum VisualFixture {
    static func decode<T: Decodable>(_ type: T.Type,_ object: Any) throws -> T { try JSONDecoder().decode(type,from: JSONSerialization.data(withJSONObject: object)) }
    static func entry(_ id: Int64,_ name: String,performer: Int = 0,composer: Int = 0,arranger: Int = 0,songs: Int = 1,type: String = "",ended: String = "") throws -> DirectoryEntry {
        try decode(DirectoryEntry.self,["id":id,"name":name,"imagePath":"","artistType":type,"ended":ended,"imageManual":false,"performer":performer,"composer":composer,"lyricist":0,"arranger":arranger,"songs":songs,"firstYear":"","lastYear":""])
    }
    static func membership(band: String = "ヨルシカ",member: String = "suis",memberId: Int = 3,left: String = "",joined: String = "2017") throws -> Membership {
        try decode(Membership.self,["bandId":1,"bandName":band,"memberId":memberId,"memberName":member,"part":"主唱","joined":joined,"leftAt":left,"source":"musicbrainz","bandImage":"","memberImage":""])
    }
    static func work(_ id: String,_ performer: Int,_ album: Int?) throws -> PersonWork {
        try decode(PersonWork.self,["id":id,"title":id,"artist":"","album":"","albumId":album as Any? ?? NSNull(),"year":"","genre":"","audioPath":"","coverPath":"","durationSec":NSNull(),"lineCount":0,"roles":["performer"],"performerIds":[performer]])
    }
    static func hit(_ song: String,repeatCount: Int = 1,utterance: Int = 1,left: String = "",keyword: String = "",text: String = "",time: Double? = nil,end: Double? = nil,endKind: String? = nil) throws -> KwicHit {
        try decode(KwicHit.self,["songId":song,"artist":"A","title":"t\(song)","audioPath":"","utteranceId":utterance,"lineIdx":0,"timeSec":time as Any? ?? NSNull(),"text":text,"left":left,"keyword":keyword,"right":"","repeatCount":repeatCount,"corrected":false,"endSec":end as Any? ?? NSNull(),"endKind":endKind as Any? ?? NSNull(),"matchLen":1])
    }
}
