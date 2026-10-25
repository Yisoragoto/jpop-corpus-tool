import XCTest
import JPCore
@testable import JPCorpusModel

/// graph.test.ts 全部 6 例；另有 Windows 数值结果对账。
final class GraphTests: XCTestCase {
    private func graph(_ options: GraphOptions = GraphOptions()) throws -> RelationGraph {
        let people = try [VisualFixture.entry(1,"ヨルシカ",performer: 10,songs: 10,type: "Group"),VisualFixture.entry(2,"n-buna",composer: 10,songs: 10),VisualFixture.entry(3,"suis",performer: 1),VisualFixture.entry(4,"西村ヒロ",arranger: 3,songs: 3),VisualFixture.entry(5,"Vaundy",performer: 5,songs: 5)]
        return GraphLayout.buildGraph(people,[CollabEdge(source: 1,target: 2,shared: 10),CollabEdge(source: 2,target: 4,shared: 2),CollabEdge(source: 4,target: 5,shared: 3)],[try VisualFixture.membership()],options)
    }
    func testMembershipWithoutCredits() throws {
        let g = try graph(), link = try XCTUnwrap(g.links.first { $0.member })
        XCTAssertEqual(Set([g.nodes[link.s].name,g.nodes[link.t].name]),["suis","ヨルシカ"]); XCTAssertTrue(g.nodes.first { $0.id == 1 }!.isBand)
    }
    func testFocusBreadthFirstDepth() throws {
        XCTAssertEqual(Set(try graph(GraphOptions(focus: 2,depth: 1)).nodes.map(\.name)),["n-buna","ヨルシカ","西村ヒロ"])
        XCTAssertEqual(Set(try graph(GraphOptions(focus: 2,depth: 2)).nodes.map(\.name)),["Vaundy","n-buna","suis","ヨルシカ","西村ヒロ"])
    }
    func testThinEdgesAndRoleFilter() throws {
        XCTAssertEqual(Set(try graph(GraphOptions(roles: ["performer"],minShared: 3)).nodes.map(\.name)),["Vaundy","suis","ヨルシカ"])
    }
    func testSimulationSettlesAndStops() throws {
        var sim = GraphSimulation(try graph()); sim.settle(); XCTAssertFalse(sim.running)
        func distance(_ a: Int64,_ b: Int64) -> Double { let x = sim.graph.nodes.first { $0.id == a }!, y = sim.graph.nodes.first { $0.id == b }!; return hypot(x.x-y.x,x.y-y.y) }
        XCTAssertLessThan(distance(1,2),distance(1,5)); XCTAssertTrue(sim.graph.nodes.allSatisfy { $0.x.isFinite && $0.y.isFinite })
    }
    func testNodeUnderPointer() throws {
        let g = try graph(), n = g.nodes[0]
        XCTAssertEqual(GraphLayout.nodeAt(g.nodes,x: n.x+1,y: n.y),0); XCTAssertEqual(GraphLayout.nodeAt(g.nodes,x: n.x+500,y: n.y+500),-1)
    }
    func testPortraitSeparationAndCappedHitRegion() throws {
        let entries = try (0..<18).map { try VisualFixture.entry(Int64($0),"Person \($0)",performer: 1,songs: $0+1) }
        var sim = GraphSimulation(GraphLayout.buildGraph(entries,entries.dropFirst().map { CollabEdge(source: 0,target: $0.id,shared: 1) },[])); sim.settle()
        let nodes = sim.graph.nodes
        for i in nodes.indices { for j in nodes.indices where j > i { XCTAssertGreaterThan(hypot(nodes[i].x-nodes[j].x,nodes[i].y-nodes[j].y),nodes[i].r+nodes[j].r+4) } }
        XCTAssertEqual(GraphLayout.nodeAt([nodes[0]],x: nodes[0].x+10,y: nodes[0].y,slop: 0,maxRadius: 5),-1)
    }
    func testWindowsSettledCoordinates() throws {
        var sim = GraphSimulation(try graph()); sim.settle()
        let expected = [[59.20429537125351,-18.06669029561331],[-32.70480037875759,35.952775010582116],[19.548752194106818,44.20872034240233],[19.906382057264853,-55.106401998971606],[-66.96275819039252,-6.621072483026579]]
        for (i,n) in sim.graph.nodes.enumerated() { XCTAssertEqual(n.x,expected[i][0],accuracy: 1e-8); XCTAssertEqual(n.y,expected[i][1],accuracy: 1e-8) }
    }
}
