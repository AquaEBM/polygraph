use super::*;
use core::{array, convert::identity, ops::Not};

fn insert_success(graph: &mut Graph, from: (NodeID, OutputID), to: (&NodeID, &InputID)) {
    assert!(graph.try_insert_edge(from, to).is_ok_and(identity))
}

// These tests aren't ideal, I have to print the compiled schedule and review it first,
// then insert it in the final assert directive if it's correct (TODO). This is inconvenient,
// since there are often many correct schedules, and any update to the traversal order used by the
// scheduler (optimizations, different hashing algorithms...) will break these tests, despite
// still creating correct schedules.

#[test]
fn basic_cycle() {
    let mut graph = Graph::default();

    let (node1_id, node1) = graph.insert_node();
    let node1_input_id = node1.add_input();
    let node1_output_id = node1.add_output(0);

    assert!(graph
        .try_insert_edge((node1_id, node1_output_id), (&node1_id, &node1_input_id))
        .is_err_and(identity))
}

#[test]
fn insert_redundant_edge() {
    let mut graph = Graph::default();

    let (node1_id, node1) = graph.insert_node();
    let node1_output = node1.add_output(0);

    let (node2_id, node2) = graph.insert_node();
    let node2_input = node2.add_input();

    insert_success(
        &mut graph,
        (node1_id, node1_output),
        (&node2_id, &node2_input),
    );
    assert!(graph
        .try_insert_edge((node1_id, node1_output), (&node2_id, &node2_input))
        .is_ok_and(Not::not));
}

#[test]
fn basic() {
    let mut graph = Graph::default();

    let (source_id, source) = graph.insert_node();
    let source_output_id = source.add_output(5);

    let (master_id, master) = graph.insert_node();
    let master_input_id = master.add_input();

    insert_success(
        &mut graph,
        (source_id, source_output_id),
        (&master_id, &master_input_id),
    );

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(&master_id);

    let schedule = scheduler.compile();

    println!("{:#?}", scheduler.intermediate());
    println!("{schedule:#?}");
}

#[test]
fn chain() {
    let mut graph = Graph::default();

    let (source_id, source) = graph.insert_node();
    let node1_output_id = source.add_output(4);

    let (int1_id, int1) = graph.insert_node();
    let int1_output_id = int1.add_output(6);
    let int1_input_id = int1.add_input();

    let (int2_id, int2) = graph.insert_node();
    let int2_output_id = int2.add_output(9);
    let int2_input_id = int2.add_input();

    let (master_id, master) = graph.insert_node();
    let master_input_id = master.add_input();

    insert_success(
        &mut graph,
        (source_id, node1_output_id),
        (&int1_id, &int1_input_id),
    );
    insert_success(
        &mut graph,
        (int1_id, int1_output_id),
        (&int2_id, &int2_input_id),
    );
    insert_success(
        &mut graph,
        (int2_id, int2_output_id),
        (&master_id, &master_input_id),
    );

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(&master_id);

    let schedule = scheduler.compile();

    println!("{:#?}", scheduler.intermediate());
    println!("{schedule:#?}");
}

#[test]
fn one_output_many_input_nodes() {
    let mut graph = Graph::default();

    let (source_id, source) = graph.insert_node();
    let source_output_id = source.add_output(10);

    let master: [_; 4] = array::from_fn(|_| {
        let (node_id, node) = graph.insert_node();
        let input_id = node.add_input();

        insert_success(
            &mut graph,
            (source_id, source_output_id),
            (&node_id, &input_id),
        );

        (node_id, input_id)
    });

    let mut scheduler = graph.scheduler();

    for (id, _) in &master {
        scheduler.add_sink_node(id);
    }

    let schedule = scheduler.compile();

    println!("{:#?}", scheduler.intermediate());
    println!("{schedule:#?}");
}

#[test]
fn adders() {
    let mut graph = Graph::default();
    let latencies = [6, 8, 13];

    let sources = latencies.map(|lat| {
        let (source_id, source) = graph.insert_node();
        (source_id, source.add_output(lat))
    });

    let (sink_id, sink) = graph.insert_node();
    let sink_input_id = sink.add_input();

    for (node_id, output_id) in sources {
        insert_success(&mut graph, (node_id, output_id), (&sink_id, &sink_input_id));
    }

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(&sink_id);

    let schedule = scheduler.compile();

    println!("{:#?}", scheduler.intermediate());
    println!("{schedule:#?}");
}

#[test]
fn w_graph() {
    let mut graph = Graph::default();

    let (left_leg_id, left_leg) = graph.insert_node();
    let left_foot_id = left_leg.add_output(15);
    let (right_leg_id, right_leg) = graph.insert_node();
    let right_foot_id = right_leg.add_output(10);

    let (left_arm_id, left_arm) = graph.insert_node();
    let left_hand_id = left_arm.add_input();
    let (head_id, head) = graph.insert_node();
    let nose_id = head.add_input();
    let (right_arm_id, right_arm) = graph.insert_node();
    let right_hand_id = right_arm.add_input();

    insert_success(
        &mut graph,
        (left_leg_id, left_foot_id),
        (&left_arm_id, &left_hand_id),
    );
    insert_success(
        &mut graph,
        (left_leg_id, left_foot_id),
        (&head_id, &nose_id),
    );
    insert_success(
        &mut graph,
        (right_leg_id, right_foot_id),
        (&head_id, &nose_id),
    );
    insert_success(
        &mut graph,
        (right_leg_id, right_foot_id),
        (&right_arm_id, &right_hand_id),
    );

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(&left_arm_id);
    scheduler.add_sink_node(&head_id);
    scheduler.add_sink_node(&right_arm_id);

    let schedule = scheduler.compile();

    println!("{:#?}", scheduler.intermediate());
    println!("{schedule:#?}");
}

// basically the transpose of the w_graph
#[test]
fn m_graph() {
    let mut graph = Graph::default();

    let (left_arm_id, left_arm) = graph.insert_node();
    let left_hand_id = left_arm.add_output(0);
    let (head_id, head) = graph.insert_node();
    let nose_id = head.add_output(0);
    let (right_arm_id, right_arm) = graph.insert_node();
    let right_hand_id = right_arm.add_output(0);

    let (left_leg_id, left_leg) = graph.insert_node();
    let left_foot_id = left_leg.add_input();
    let (right_leg_id, right_leg) = graph.insert_node();
    let right_foot_id = right_leg.add_input();

    insert_success(
        &mut graph,
        (left_arm_id, left_hand_id),
        (&left_leg_id, &left_foot_id),
    );
    insert_success(
        &mut graph,
        (head_id, nose_id),
        (&left_leg_id, &left_foot_id),
    );
    insert_success(
        &mut graph,
        (head_id, nose_id),
        (&right_leg_id, &right_foot_id),
    );
    insert_success(
        &mut graph,
        (right_arm_id, right_hand_id),
        (&right_leg_id, &right_foot_id),
    );

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(&left_leg_id);
    scheduler.add_sink_node(&right_leg_id);

    let schedule = scheduler.compile();

    println!("{:#?}", scheduler.intermediate());
    println!("{schedule:#?}");
}

#[test]
fn multiple_input_ports() {
    const NUM_INPUT_PORTS: usize = 4;

    let mut graph = Graph::default();

    let (source_id, source) = graph.insert_node();
    let source_node_output_id = source.add_output(13);

    let (master_id, master) = graph.insert_node();
    let master_input_ids: [_; NUM_INPUT_PORTS] = array::from_fn(|_| master.add_input());

    for input_id in &master_input_ids {
        insert_success(
            &mut graph,
            (source_id, source_node_output_id),
            (&master_id, input_id),
        );
    }

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(&master_id);

    let schedule = scheduler.compile();

    println!("{:#?}", scheduler.intermediate());
    println!("{schedule:#?}");
}

#[test]
fn multiple_outputs_one_input() {
    const NUM_OUTPUT_PORTS: usize = 4;

    let mut graph = Graph::default();

    let (source_id, source) = graph.insert_node();
    let source_output_id: [_; NUM_OUTPUT_PORTS] =
        array::from_fn(|i| source.add_output((i + 1) as u64 * 4));

    let (sink_id, sink) = graph.insert_node();
    let sink_input_id = sink.add_input();

    for output_id in source_output_id {
        insert_success(
            &mut graph,
            (source_id, output_id),
            (&sink_id, &sink_input_id),
        );
    }

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(&sink_id);

    let schedule = scheduler.compile();

    println!("{:#?}", scheduler.intermediate());
    println!("{schedule:#?}");
}
