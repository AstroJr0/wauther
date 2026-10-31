import QtQuick
import QtQuick.Controls
import QtQuick.Shapes
import wauther

ApplicationWindow {
    id: win

    visible: true
    width: 440
    height: 700
    minimumWidth: 340
    minimumHeight: 480
    title: "wauther"
    color: m3.surface

    property real clock: Date.now()
    property real soonest: Infinity     // when the first code runs out

    // Material 3 dark scheme, blue seed.
    QtObject {
        id: m3
        readonly property color surface: "#101418"
        readonly property color surfaceContainer: "#1D2024"
        readonly property color surfaceContainerHigh: "#272A2F"
        readonly property color surfaceContainerHighest: "#32353A"
        readonly property color onSurface: "#E0E2E8"
        readonly property color onSurfaceVariant: "#C3C7CF"
        readonly property color outline: "#8D9199"
        readonly property color outlineVariant: "#43474E"
        readonly property color primary: "#9FCAFF"
        readonly property color onPrimary: "#003258"
        readonly property color primaryContainer: "#00497D"
        readonly property color onPrimaryContainer: "#D1E4FF"
        readonly property color secondaryContainer: "#3B4858"
        readonly property color onSecondaryContainer: "#D7E3F8"
        readonly property color tertiaryContainer: "#523F5F"
        readonly property color error: "#FFB4AB"
        readonly property color onError: "#690005"
        readonly property color errorContainer: "#93000A"
        readonly property color onErrorContainer: "#FFDAD6"
        readonly property color inverseSurface: "#E0E2E8"
        readonly property color inverseOnSurface: "#2E3135"
    }

    ListModel { id: keys }

    TextEdit { id: clipboard; visible: false }

    // logic 

    function sync(json) {
        const rows = JSON.parse(json);
        let same = rows.length === keys.count;
        for (let i = 0; same && i < rows.length; i++)
            same = keys.get(i).name === rows[i].name;
        if (same) {
            rows.forEach((row, i) => keys.set(i, row));
        } else {
            keys.clear();
            rows.forEach(row => keys.append(row));
        }
        soonest = rows.reduce((least, row) => Math.min(least, row.expires), Infinity);
    }

    function say(text) {
        toastText.text = text;
        toast.shown = true;
        toastTimer.restart();
    }

    function copy(code, name) {
        clipboard.text = code;
        clipboard.selectAll();
        clipboard.copy();
        say("Copied code of " + name);
    }

    function remove(name) {
        const error = Backend.remove(name);
        if (error) {
            say(error);
        } else {
            say("Deleted " + name);
            Backend.refresh();
        }
    }

    function added(error, name) {
        if (error) {
            prompt.showError(error);
        } else {
            prompt.clear();
            prompt.close();
            Backend.refresh();
            say("Added " + name);
        }
    }

    Component.onCompleted: Backend.refresh()

    Connections {
        target: Backend
        function onRowsJsonChanged() { win.sync(Backend.rowsJson); }
    }

    // Drives the pies; asks Rust for new codes the moment one runs out.
    Timer {
        interval: 33
        repeat: true
        running: true
        onTriggered: {
            win.clock = Date.now();
            if (win.clock >= win.soonest)
                Backend.refresh();
        }
    }

    // header title

    Column {
        id: header

        anchors.top: parent.top
        anchors.topMargin: 28
        width: parent.width
        spacing: 22

        // its the white "W" on a purple badge, then the rest of the name.
        Item {
            width: header.width
            height: 48

            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: 6

                Rectangle {
                    id: badge
                    width: 48
                    height: 48
                    radius: 14
                    color: m3.tertiaryContainer

                    Text {
                        anchors.centerIn: parent
                        text: "W"
                        color: "white"
                        font.pixelSize: 32
                        font.bold: true
                    }
                    // Tungsten's atomic number, like a cell of the periodic table.
                    Text {
                        anchors.left: parent.left
                        anchors.bottom: parent.bottom
                        anchors.leftMargin: 6
                        anchors.bottomMargin: 6
                        text: "74"
                        color: "white"
                        opacity: 0.7
                        font.pixelSize: 10
                    }
                }
                Text {
                    anchors.verticalCenter: badge.verticalCenter
                    text: "Auther"
                    color: m3.onSurface
                    font.pixelSize: 34
                    font.bold: true
                }
            }
        }

        // seperator
        Item {
            width: header.width
            height: 40

            HoverHandler { id: dotsHover }

            Row {
                anchors.centerIn: parent
                spacing: 12

                Repeater {
                    model: 3

                    Rectangle {
                        id: dot

                        required property int index
                        property real lit: 0

                        width: 14
                        height: 14
                        radius: 7
                        color: Qt.tint(m3.outlineVariant, Qt.alpha(m3.primary, lit))

                        SequentialAnimation {
                            running: dotsHover.hovered
                            loops: Animation.Infinite
                            alwaysRunToEnd: true

                            PauseAnimation { duration: dot.index * 480 }
                            NumberAnimation { target: dot; property: "lit"; to: 1; duration: 600; easing.type: Easing.InOutSine }
                            NumberAnimation { target: dot; property: "lit"; to: 0; duration: 600; easing.type: Easing.InOutSine }
                            PauseAnimation { duration: 1200 - dot.index * 480 }
                        }
                    }
                }
            }
        }
    }

    // list view of the keys

    ListView {
        id: list

        anchors.top: header.bottom
        anchors.topMargin: 12
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        anchors.bottomMargin: 112
        spacing: 2
        clip: true
        model: keys

        delegate: Item {
            id: row

            required property int index
            required property string name
            required property string code
            required property bool valid
            required property real expires
            required property real period

            readonly property bool hovered: hover.hovered
            readonly property real secs: Math.max(0, (expires - win.clock) / 1000)
            readonly property real fraction: Math.min(1, secs / period)
            readonly property bool low: valid && secs <= 5
            // Outermost corners are the roundest, inner ones are tight.
            readonly property int topRadius: index === 0 ? 28 : 6
            readonly property int bottomRadius: index === keys.count - 1 ? 28 : 6
            readonly property string spaced: {
                const half = Math.floor(code.length / 2);
                return code.slice(0, half) + " " + code.slice(half);
            }
            property bool armed: false      // delete was pressed once

            width: ListView.view.width
            height: 72

            HoverHandler { id: hover }
            onHoveredChanged: if (!hovered) armed = false

            Rectangle {
                anchors.fill: parent
                topLeftRadius: row.topRadius
                topRightRadius: row.topRadius
                bottomLeftRadius: row.bottomRadius
                bottomRightRadius: row.bottomRadius
                color: row.hovered ? m3.surfaceContainerHigh : m3.surfaceContainer
                Behavior on color { ColorAnimation { duration: 150 } }
            }

            MouseArea {
                anchors.fill: parent
                enabled: row.valid && !prompt.opened
                cursorShape: Qt.PointingHandCursor
                onClicked: win.copy(row.code, row.name)
            }

            Text {
                x: 24
                anchors.verticalCenter: parent.verticalCenter
                width: Math.max(0, row.width / 2 - codeText.width / 2 - 12 - 24)
                text: row.name
                elide: Text.ElideRight
                color: m3.onSurface
                font.pixelSize: 17
            }

            Text {
                id: codeText
                anchors.centerIn: parent
                text: row.valid ? row.spaced : "invalid key"
                font.family: "monospace"
                font.pixelSize: row.valid ? 30 : 18
                font.bold: true
                color: row.low || !row.valid ? m3.error : m3.primary
                Behavior on color { ColorAnimation { duration: 200 } }
            }

            // How much of the period is left, as a pie.
            Item {
                id: pie
                width: 28
                height: 28
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: parent.right
                anchors.rightMargin: 22 + del.width

                Rectangle {
                    anchors.fill: parent
                    radius: 14
                    color: m3.secondaryContainer
                }
                Shape {
                    anchors.fill: parent
                    visible: row.valid && row.fraction > 0.001
                    preferredRendererType: Shape.CurveRenderer

                    ShapePath {
                        fillColor: row.low ? m3.error : m3.primary
                        strokeColor: "transparent"
                        startX: 14
                        startY: 14

                        PathLine { x: 14; y: 0 }
                        PathAngleArc {
                            centerX: 14
                            centerY: 14
                            radiusX: 14
                            radiusY: 14
                            startAngle: -90
                            sweepAngle: Math.min(359.9, 360 * row.fraction)
                        }
                        PathLine { x: 14; y: 14 }
                    }
                }
            }

            Rectangle {
                id: del

                anchors.right: parent.right
                height: parent.height
                width: row.hovered && !prompt.opened ? 68 : 0
                visible: width > 1
                topLeftRadius: 0
                bottomLeftRadius: 0
                topRightRadius: row.topRadius
                bottomRightRadius: row.bottomRadius
                color: row.armed ? m3.error : m3.errorContainer

                Behavior on width { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
                Behavior on color { ColorAnimation { duration: 120 } }

                Concave {
                    x: -radius
                    y: 0
                    radius: Math.round(16 * del.width / 68)
                    color: del.color
                    isTop: true
                    mirrored: true
                }
                Concave {
                    x: -radius
                    y: del.height - radius
                    radius: Math.round(16 * del.width / 68)
                    color: del.color
                    isTop: false
                    mirrored: true
                }

                Item {
                    anchors.centerIn: parent
                    visible: !row.armed
                    opacity: del.width / 68

                    Rectangle {
                        anchors.centerIn: parent
                        width: 18; height: 2.5; radius: 1.25
                        rotation: 45
                        color: m3.onErrorContainer
                    }
                    Rectangle {
                        anchors.centerIn: parent
                        width: 18; height: 2.5; radius: 1.25
                        rotation: -45
                        color: m3.onErrorContainer
                    }
                }
                Shape {
                    x: parent.width / 2 - 9
                    y: parent.height / 2 - 7
                    visible: row.armed
                    preferredRendererType: Shape.CurveRenderer

                    ShapePath {
                        strokeColor: m3.onError
                        strokeWidth: 3
                        fillColor: "transparent"
                        capStyle: ShapePath.RoundCap
                        joinStyle: ShapePath.RoundJoin
                        startX: 0
                        startY: 8
                        PathLine { x: 6; y: 14 }
                        PathLine { x: 18; y: 0 }
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: row.armed ? win.remove(row.name) : (row.armed = true)
                }
                Timer {
                    running: row.armed
                    interval: 3000
                    onTriggered: row.armed = false
                }
            }
        }
    }

    Column {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: header.bottom
        anchors.topMargin: 64
        visible: keys.count === 0
        spacing: 8

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: "No keys yet"
            color: m3.onSurfaceVariant
            font.pixelSize: 18
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: "Press + to add one"
            color: m3.outline
            font.pixelSize: 14
        }
    }

    // fab
    Item {
        id: fab

        width: 88
        height: 88
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        visible: prompt.progress < 0.01

        readonly property color fill: m3.primaryContainer

        Rectangle {
            anchors.fill: parent
            topLeftRadius: 28
            color: fab.fill

            Rectangle {
                anchors.fill: parent
                topLeftRadius: 28
                color: m3.onPrimaryContainer
                opacity: fabHover.hovered ? 0.12 : 0
                Behavior on opacity { NumberAnimation { duration: 150 } }
            }
        }
        Concave {
            x: fab.width - radius
            y: -radius
            radius: 16
            color: fab.fill
            isTop: false
            mirrored: true
        }
        Concave {
            x: -radius
            y: fab.height - radius
            radius: 16
            color: fab.fill
            isTop: false
            mirrored: true
        }

        Rectangle {
            anchors.centerIn: parent
            width: 30; height: 6; radius: 3
            color: m3.onPrimaryContainer
        }
        Rectangle {
            anchors.centerIn: parent
            width: 6; height: 30; radius: 3
            color: m3.onPrimaryContainer
        }

        HoverHandler { id: fabHover }
        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: prompt.open()
        }
    }

    // toast

    Rectangle {
        id: toast

        property bool shown: false

        x: 16
        y: parent.height - height - 24
        width: toastText.width + 32
        height: toastText.height + 24
        radius: 12
        color: m3.inverseSurface
        opacity: shown ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 200 } }

        Text {
            id: toastText
            anchors.centerIn: parent
            color: m3.inverseOnSurface
            font.pixelSize: 14
        }
        Timer {
            id: toastTimer
            interval: 2000
            onTriggered: toast.shown = false
        }
    }

    // prompt
    Prompt {
        id: prompt

        theme: m3
        onAddManual: (name, key) => win.added(Backend.add(name, key), name.trim())
        onAddUri: (uri, name) => win.added(Backend.addUri(uri, name), name || "key")
    }
}
