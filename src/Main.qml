pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Shapes
import QtQuick.Layouts

ApplicationWindow {
    id: root

    // The Rust side: rowsJson, error, refresh(), add(name, key), addUri(uri, name), remove(name).
    required property var backend

    function copy(code, name) {
        clipboard.text = code;
        clipboard.selectAll();
        clipboard.copy();
        toast.show(qsTr("Copied code of %1").arg(name));
    }

    function remove(name) {
        backend.remove(name);
        toast.show(backend.error || qsTr("Deleted %1").arg(name));
        backend.refresh();
    }

    function added(name) {
        prompt.error = backend.error;
        if (prompt.error)
            return;
        prompt.clear();
        prompt.close();
        backend.refresh();
        toast.show(qsTr("Added %1").arg(name));
    }

    visible: true
    width: 440
    height: 700
    minimumWidth: 340
    minimumHeight: 480
    title: "wauther"
    color: theme.surface

    QtObject {
        id: theme

        readonly property color surface: "#101418"
        readonly property color surfaceContainer: "#1D2024"
        readonly property color surfaceContainerHigh: "#272A2F"
        readonly property color surfaceContainerHighest: "#32353A"
        readonly property color surfaceText: "#E0E2E8"
        readonly property color surfaceVariantText: "#C3C7CF"
        readonly property color outline: "#8D9199"
        readonly property color primary: "#9FCAFF"
        readonly property color primaryText: "#003258"
        readonly property color primaryContainer: "#00497D"
        readonly property color primaryContainerText: "#D1E4FF"
        readonly property color secondaryContainer: "#3B4858"
        readonly property color secondaryContainerText: "#D7E3F8"
        readonly property color tertiaryContainer: "#523F5F"
        readonly property color error: "#FFB4AB"
        readonly property color errorText: "#690005"
        readonly property color errorContainer: "#93000A"
        readonly property color errorContainerText: "#FFDAD6"
        readonly property color inverseSurface: "#E0E2E8"
        readonly property color inverseText: "#2E3135"
        readonly property color outlineVariant: "#43474E"
    }

    Component.onCompleted: backend.refresh()

    Connections {
        function onRowsJsonChanged() {
            list.sync(root.backend.rowsJson);
        }

        target: root.backend
    }

    TextEdit {
        id: clipboard
        visible: false
    }

    Header {
        id: header
        width: parent.width
        anchors {
            top: parent.top
            topMargin: 28
        }
    }

    KeyList {
        id: list

        enabled: !prompt.opened
        anchors {
            top: header.bottom
            topMargin: 12
            bottom: parent.bottom
            bottomMargin: 112
            left: parent.left
            leftMargin: 16
            right: parent.right
            rightMargin: 16
        }

        onRefreshNeeded: root.backend.refresh()
        onCopyRequested: (code, name) => root.copy(code, name)
        onDeleteRequested: name => root.remove(name)
    }

    Column {
        visible: list.count === 0
        spacing: 8
        anchors {
            horizontalCenter: parent.horizontalCenter
            top: header.bottom
            topMargin: 64
        }

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: qsTr("No keys yet")
            color: theme.surfaceVariantText
            font.pixelSize: 18
        }

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: qsTr("Press + to add one")
            color: theme.outline
            font.pixelSize: 14
        }
    }

    Fab {
        visible: prompt.progress < 0.01
        anchors {
            right: parent.right
            bottom: parent.bottom
        }

        onClicked: prompt.open()
    }

    Toast {
        id: toast
        anchors {
            left: parent.left
            leftMargin: 16
            bottom: parent.bottom
            bottomMargin: 24
        }
    }

    Prompt {
        id: prompt

        anchors.fill: parent
        z: 1

        onAddManual: (name, key) => {
            root.backend.add(name, key);
            root.added(name.trim());
        }
        onAddUri: (uri, name) => {
            root.backend.addUri(uri, name);
            root.added(name || qsTr("key"));
        }
    }

    component Concave: Shape {
        id: root

        property int radius: 14
        property color color: "#1e1e2d"
        property bool isTop: true
        property bool mirrored: false   // true = horizontally flip (for right-side bars)

        implicitWidth: radius
        implicitHeight: radius

        preferredRendererType: Shape.CurveRenderer

        transform: Scale {
    	xScale: root.mirrored ? -1 : 1
    	origin.x: root.radius / 2
    	origin.y: 0
        }

        ShapePath {
    	fillColor: root.color
    	strokeColor: "transparent"

    	startX: root.isTop ? root.radius : 0
    	startY: root.isTop ? 0 : 0

    	PathLine {
    	    x: 0
    	    y: root.isTop ? 0 : root.radius
    	}

    	PathLine {
    	    x: root.isTop ? 0 : root.radius
    	    y: root.isTop ? root.radius : root.radius
    	}

    	PathArc {
    	    x: root.isTop ? root.radius : 0
    	    y: root.isTop ? 0 : 0
    	    radiusX: root.radius
    	    radiusY: root.radius
    	    direction: root.isTop ? PathArc.CounterClockwise : PathArc.Clockwise
    	}

        }

    }

    // A disc, filled clockwise from 12 o'clock over `fraction` of the circle.
    component Pie: Item {
        id: root

        property real fraction
        property color color

        readonly property real r: width / 2

        implicitWidth: 28
        implicitHeight: 28

        Rectangle {
            anchors.fill: parent
            radius: root.r
            color: theme.secondaryContainer
        }

        Shape {
            anchors.fill: parent
            visible: root.fraction > 0.001
            preferredRendererType: Shape.CurveRenderer

            ShapePath {
                fillColor: root.color
                strokeColor: "transparent"
                startX: root.r
                startY: root.r

                PathLine {
                    x: root.r
                    y: 0
                }
                PathAngleArc {
                    centerX: root.r
                    centerY: root.r
                    radiusX: root.r
                    radiusY: root.r
                    startAngle: -90
                    sweepAngle: Math.min(359.9, 360 * root.fraction)
                }
            }
        }
    }

    // Slides in from the right, joined to its row by two concave curves.
    component DeleteButton: Rectangle {
        id: root

        property bool shown
        property bool armed

        readonly property int curve: Math.round(16 * width / 68)

        signal clicked

        width: shown ? 68 : 0
        visible: width > 1
        topLeftRadius: 0
        bottomLeftRadius: 0
        color: armed ? theme.error : theme.errorContainer

        Behavior on width {
            NumberAnimation {
                duration: 180
                easing.type: Easing.OutCubic
            }
        }

        Concave {
            x: -radius
            radius: root.curve
            color: root.color
            isTop: true
            mirrored: true
        }

        Concave {
            x: -radius
            y: root.height - radius
            radius: root.curve
            color: root.color
            isTop: false
            mirrored: true
        }

        Repeater {
            model: [45, -45]

            Rectangle {
                required property int modelData

                anchors.centerIn: parent
                visible: !root.armed
                width: 18
                height: 2.5
                radius: 1.25
                rotation: modelData
                color: theme.errorContainerText
            }
        }

        Shape {
            x: parent.width / 2 - 9
            y: parent.height / 2 - 7
            visible: root.armed
            preferredRendererType: Shape.CurveRenderer

            ShapePath {
                strokeColor: theme.errorText
                strokeWidth: 3
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                joinStyle: ShapePath.RoundJoin
                startY: 8

                PathLine {
                    x: 6
                    y: 14
                }
                PathLine {
                    x: 18
                }
            }
        }

        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: root.clicked()
        }
    }

    component KeyRow: Item {
        id: root

        required property int index
        required property int count
        required property string name
        required property string code
        required property bool valid
        required property real expires
        required property real period
        required property real clock

        readonly property real secs: Math.max(0, (expires - clock) / 1000)
        readonly property bool low: valid && secs <= 5
        readonly property int topRadius: index === 0 ? 28 : 6
        readonly property int bottomRadius: index === count - 1 ? 28 : 6
        property bool armed: false      // delete was pressed once

        signal copyRequested
        signal deleteRequested

        height: 72

        HoverHandler {
            id: hover
            onHoveredChanged: if (!hovered) root.armed = false
        }

        Rectangle {
            anchors.fill: parent
            topLeftRadius: root.topRadius
            topRightRadius: root.topRadius
            bottomLeftRadius: root.bottomRadius
            bottomRightRadius: root.bottomRadius
            color: hover.hovered ? theme.surfaceContainerHigh : theme.surfaceContainer
        }

        MouseArea {
            anchors.fill: parent
            enabled: root.valid
            cursorShape: Qt.PointingHandCursor
            onClicked: root.copyRequested()
        }

        Text {
            width: Math.max(0, root.width / 2 - codeText.width / 2 - 36)
            text: root.name
            elide: Text.ElideRight
            color: theme.surfaceText
            font.pixelSize: 17
            anchors {
                left: parent.left
                leftMargin: 24
                verticalCenter: parent.verticalCenter
            }
        }

        Text {
            id: codeText

            anchors.centerIn: parent
            text: root.valid ? root.code.slice(0, root.code.length / 2) + " " + root.code.slice(root.code.length / 2) : qsTr("invalid key")
            color: root.low || !root.valid ? theme.error : theme.primary
            font {
                family: "monospace"
                pixelSize: root.valid ? 30 : 18
                bold: true
            }
        }

        Pie {
            fraction: root.valid ? Math.min(1, root.secs / root.period) : 0
            color: root.low ? theme.error : theme.primary
            anchors {
                right: parent.right
                rightMargin: 22 + del.width
                verticalCenter: parent.verticalCenter
            }
        }

        // First press arms it, the second deletes. It disarms after 3s.
        DeleteButton {
            id: del

            shown: hover.hovered
            armed: root.armed
            topRightRadius: root.topRadius
            bottomRightRadius: root.bottomRadius
            height: parent.height
            anchors.right: parent.right

            onClicked: root.armed ? root.deleteRequested() : root.armed = true
        }

        Timer {
            running: root.armed
            interval: 3000
            onTriggered: root.armed = false
        }
    }

    // The keys, as a Material table: outermost corners round, inner ones tight.
    component KeyList: ListView {
        id: root

        property real clock
        property real soonest: Infinity     // when the first code runs out

        signal refreshNeeded
        signal copyRequested(string code, string name)
        signal deleteRequested(string name)

        // rows: [{ name, code, valid, expires (unix ms), period (s) }]
        function sync(json) {
            const rows = JSON.parse(json);
            if (rows.length === keys.count && rows.every((row, i) => row.name === keys.get(i).name)) {
                rows.forEach((row, i) => keys.set(i, row));
            } else {
                keys.clear();
                rows.forEach(row => keys.append(row));
            }
            soonest = Math.min(...rows.map(row => row.expires));
        }

        spacing: 2
        clip: true
        model: ListModel {
            id: keys
        }

        delegate: KeyRow {
            width: ListView.view.width
            count: root.count
            clock: root.clock

            onCopyRequested: root.copyRequested(code, name)
            onDeleteRequested: root.deleteRequested(name)
        }

        // Drives the pies, and asks for new codes the moment one runs out.
        Timer {
            interval: 33
            repeat: true
            running: root.visible
            triggeredOnStart: true
            onTriggered: {
                root.clock = Date.now();
                if (root.clock >= root.soonest)
                    root.refreshNeeded();
            }
        }
    }

    component Header: Column {
        id: root

        spacing: 22

        // White W on a purple badge, then the rest of the name.
        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: 6

            Rectangle {
                id: badge

                width: 48
                height: 48
                radius: 14
                color: theme.tertiaryContainer

                Text {
                    anchors.centerIn: parent
                    text: "W"
                    color: "white"
                    font {
                        pixelSize: 32
                        bold: true
                    }
                }

                // Tungsten's atomic number, like a periodic table cell.
                Text {
                    text: "74"
                    color: "#b3ffffff"
                    font.pixelSize: 10
                    anchors {
                        left: parent.left
                        bottom: parent.bottom
                        margins: 6
                    }
                }
            }

            Text {
                anchors.verticalCenter: badge.verticalCenter
                text: "Auther"
                color: theme.surfaceText
                font {
                    pixelSize: 34
                    bold: true
                }
            }
        }

        // Three dots: still until hovered, then they blink one after another.
        Item {
            width: parent.width
            height: 40

            HoverHandler {
                id: hover
            }

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
                        color: Qt.tint(theme.outlineVariant, Qt.alpha(theme.primary, lit))

                        SequentialAnimation {
                            running: hover.hovered && root.visible
                            loops: Animation.Infinite
                            alwaysRunToEnd: true

                            PauseAnimation {
                                duration: dot.index * 480
                            }
                            NumberAnimation {
                                target: dot
                                property: "lit"
                                to: 1
                                duration: 600
                                easing.type: Easing.InOutSine
                            }
                            NumberAnimation {
                                target: dot
                                property: "lit"
                                to: 0
                                duration: 600
                                easing.type: Easing.InOutSine
                            }
                            PauseAnimation {
                                duration: 1200 - dot.index * 480
                            }
                        }
                    }
                }
            }
        }
    }

    // The big +, in the bottom-right corner, joined to the window edges by two concave curves.
    component Fab: Item {
        id: root

        readonly property color fill: Qt.tint(theme.primaryContainer, Qt.alpha(theme.primaryContainerText, hover.hovered ? 0.12 : 0))

        signal clicked

        implicitWidth: 88
        implicitHeight: 88

        HoverHandler {
            id: hover
            cursorShape: Qt.PointingHandCursor
        }

        TapHandler {
            onTapped: root.clicked()
        }

        Rectangle {
            anchors.fill: parent
            topLeftRadius: 28
            color: root.fill
        }

        Concave {
            x: root.width - radius
            y: -radius
            radius: 16
            color: root.fill
            isTop: false
            mirrored: true
        }

        Concave {
            x: -radius
            y: root.height - radius
            radius: 16
            color: root.fill
            isTop: false
            mirrored: true
        }

        Repeater {
            model: [Qt.size(30, 6), Qt.size(6, 30)]

            Rectangle {
                required property size modelData

                anchors.centerIn: parent
                width: modelData.width
                height: modelData.height
                radius: 3
                color: theme.primaryContainerText
            }
        }
    }

    component Toast: Rectangle {
        id: root

        property string message

        function show(text) {
            message = text;
            hide.restart();
        }

        width: label.width + 32
        height: label.height + 24
        radius: 12
        visible: hide.running
        color: theme.inverseSurface

        Text {
            id: label

            anchors.centerIn: parent
            text: root.message
            color: theme.inverseText
            font.pixelSize: 14
        }

        Timer {
            id: hide
            interval: 2000
        }
    }

    component PromptSegment: TabButton {
        id: segment

        font {
            pixelSize: 15
            weight: Font.Medium
        }
        contentItem: Text {
            text: segment.text
            font: segment.font
            color: segment.checked ? theme.secondaryContainerText : theme.surfaceVariantText
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: 18
            color: segment.checked ? theme.secondaryContainer : theme.surfaceContainerHigh
        }
    }

    component PromptField: TextField {
        id: field

        property string label

        Layout.fillWidth: true
        Layout.preferredHeight: 56
        topPadding: 24
        leftPadding: 16
        rightPadding: 16
        bottomPadding: 6
        font.pixelSize: 16
        color: theme.surfaceText
        selectionColor: theme.primaryContainer
        placeholderTextColor: theme.outline

        onAccepted: root.submit()
        onTextEdited: root.error = ""

        background: Rectangle {
            topLeftRadius: 12
            topRightRadius: 12
            bottomLeftRadius: 4
            bottomRightRadius: 4
            color: theme.surfaceContainerHighest

            Text {
                x: 16
                y: 8
                text: field.label
                font.pixelSize: 12
                color: field.activeFocus ? theme.primary : theme.surfaceVariantText
            }

            Rectangle {
                width: parent.width
                height: field.activeFocus ? 2 : 1
                anchors.bottom: parent.bottom
                color: field.activeFocus ? theme.primary : theme.surfaceVariantText
            }
        }
    }

    component PromptPill: Button {
        id: pill

        property bool filled

        Layout.preferredWidth: 96
        Layout.preferredHeight: 40
        contentItem: Text {
            text: pill.text
            color: pill.filled ? theme.primaryText : theme.primary
            font {
                pixelSize: 15
                weight: Font.Medium
            }
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: 20
            color: Qt.tint(pill.filled ? theme.primary : theme.surfaceContainerHigh,
                           Qt.alpha(pill.filled ? theme.primaryText : theme.primary, pill.hovered ? 0.12 : 0))
        }
    }

    // Slides down from the top and covers half the window.
    // It only emits signals; the parent answers by setting `error`, or by calling clear() and close().
    component Prompt: Item {
        id: root

        property bool opened
        property real progress: opened ? 1 : 0
        property string error
        readonly property bool manual: tabs.currentIndex === 0

        signal addManual(string name, string key)
        signal addUri(string uri, string name)

        function open() {
            error = "";
            opened = true;
            Qt.callLater(focusFirst);
        }

        function close() {
            opened = false;
        }

        function clear() {
            [nameField, keyField, uriField, uriNameField].forEach(field => field.clear());
        }

        function focusFirst() {
            (manual ? nameField : uriField).forceActiveFocus();
        }

        function submit() {
            error = "";
            if (manual)
                addManual(nameField.text, keyField.text.trim());
            else
                addUri(uriField.text.trim(), uriNameField.text.trim());
        }

        visible: progress > 0.001

        Behavior on progress {
            NumberAnimation {
                duration: 300
                easing.type: Easing.OutCubic
            }
        }

        Shortcut {
            sequence: "Esc"
            enabled: root.opened
            onActivated: root.close()
        }

        // Filled text field: small label on top, text under it, indicator along the bottom.

        Rectangle {
            anchors.fill: parent
            color: Qt.rgba(0, 0, 0, 0.5 * root.progress)

            TapHandler {
                onTapped: root.close()
            }
        }

        Rectangle {
            id: panel

            width: parent.width
            height: Math.min(parent.height, Math.max(parent.height / 2, 320))
            y: -height * (1 - root.progress)
            color: theme.surfaceContainerHigh

            // Keeps clicks on the panel from reaching the scrim.
            MouseArea {
                anchors.fill: parent
            }

            // The curves joining the panel's bottom edge to the window's sides.
            Concave {
                y: panel.height
                radius: 28
                color: panel.color
                isTop: true
            }

            Concave {
                x: panel.width - radius
                y: panel.height
                radius: 28
                color: panel.color
                isTop: true
                mirrored: true
            }

            ColumnLayout {
                spacing: 12
                anchors {
                    fill: parent
                    margins: 24
                    bottomMargin: 20
                }

                TabBar {
                    id: tabs

                    Layout.fillWidth: true
                    Layout.preferredHeight: 40
                    Layout.bottomMargin: 4
                    padding: 2
                    background: Rectangle {
                        radius: 20
                        color: theme.surfaceContainerHigh
                        border.color: theme.outline
                    }

                    onCurrentIndexChanged: {
                        root.error = "";
                        Qt.callLater(root.focusFirst);
                    }

                    PromptSegment {
                        text: qsTr("Add manually")
                    }
                    PromptSegment {
                        text: qsTr("From URI")
                    }
                }

                StackLayout {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 124
                    currentIndex: tabs.currentIndex

                    ColumnLayout {
                        spacing: 12

                        PromptField {
                            id: nameField
                            onAccepted: root.submit()
                            onTextEdited: root.error = ""
                            label: qsTr("Key name")
                            placeholderText: qsTr("e.g. GitHub")
                        }
                        PromptField {
                            id: keyField
                            onAccepted: root.submit()
                            onTextEdited: root.error = ""
                            label: qsTr("Key")
                            placeholderText: qsTr("Base32 secret")
                        }
                    }

                    ColumnLayout {
                        spacing: 12

                        PromptField {
                            id: uriField
                            onAccepted: root.submit()
                            onTextEdited: root.error = ""
                            label: qsTr("URI")
                            placeholderText: "otpauth://totp/…"
                        }
                        PromptField {
                            id: uriNameField
                            onAccepted: root.submit()
                            onTextEdited: root.error = ""
                            label: qsTr("Key name")
                            placeholderText: qsTr("Taken from the URI if empty")
                        }
                    }
                }

                Text {
                    Layout.fillWidth: true
                    visible: root.error !== ""
                    text: root.error
                    wrapMode: Text.WordWrap
                    font.pixelSize: 13
                    color: theme.error
                }

                Item {
                    Layout.fillHeight: true
                }

                RowLayout {
                    Layout.alignment: Qt.AlignRight
                    spacing: 8

                    PromptPill {
                        text: qsTr("Cancel")
                        onClicked: root.close()
                    }
                    PromptPill {
                        text: qsTr("Add")
                        filled: true
                        onClicked: root.submit()
                    }
                }
            }
        }
    }
}
